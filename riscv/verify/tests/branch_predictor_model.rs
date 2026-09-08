#![allow(dead_code)]

//! Deterministic executable specification for the branch predictor.
//!
//! This test deliberately has no RTL or third-party dependencies.  It fixes the
//! geometry and the subtle state-transition rules before the Veryl interface is
//! connected to a simulator test.

const BASE_ENTRIES: usize = 2048;
const TAGGED_TABLES: usize = 4;
const TAGGED_ENTRIES: usize = 512;
const HISTORY_LENGTHS: [usize; TAGGED_TABLES] = [4, 12, 36, 108];
const TAG_WIDTHS: [usize; TAGGED_TABLES] = [8, 8, 9, 9];
const MAX_HISTORY: usize = 108;

const BTB_SETS: usize = 128;
const BTB_WAYS: usize = 2;
const RAS_DEPTH: usize = 16;
const COLD_SCRUB_CYCLES: usize = BASE_ENTRIES;

fn bit_mask(width: usize) -> u128 {
    if width == 128 {
        u128::MAX
    } else {
        (1_u128 << width) - 1
    }
}

fn conditional_instruction_hint(raw_instruction: u32, is_compressed: bool) -> bool {
    if is_compressed {
        (raw_instruction & 0b11) == 0b01 && matches!((raw_instruction >> 13) & 0b111, 0b110 | 0b111)
    } else {
        (raw_instruction & 0x7f) == 0b1100011
            && matches!(
                (raw_instruction >> 12) & 0b111,
                0b000 | 0b001 | 0b100 | 0b101 | 0b110 | 0b111
            )
    }
}

fn saturating_unsigned(value: u8, taken: bool, maximum: u8) -> u8 {
    if taken {
        value.saturating_add(1).min(maximum)
    } else {
        value.saturating_sub(1)
    }
}

fn saturating_signed(value: i8, taken: bool, minimum: i8, maximum: i8) -> i8 {
    if taken {
        value.saturating_add(1).min(maximum)
    } else {
        value.saturating_sub(1).max(minimum)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fold {
    history_length: usize,
    width: usize,
    value: u16,
}

impl Fold {
    fn new(history_length: usize, width: usize, history: u128) -> Self {
        Self {
            history_length,
            width,
            value: Self::full(history, history_length, width),
        }
    }

    fn full(history: u128, history_length: usize, width: usize) -> u16 {
        assert!(history_length <= 128);
        assert!((1..=16).contains(&width));
        let mut result = 0_u16;
        for position in 0..history_length {
            if ((history >> position) & 1) != 0 {
                result ^= 1_u16 << (position % width);
            }
        }
        result
    }

    /// Incremental form used by the RTL contract.  Bit zero is the newest GHR
    /// bit and `old_history[L - 1]` is sampled before the append.
    fn append(&mut self, old_history: u128, new_bit: bool) {
        let outgoing = ((old_history >> (self.history_length - 1)) & 1) as u32;
        let mut next = (u32::from(self.value) << 1) ^ u32::from(new_bit);
        next ^= outgoing << (self.history_length % self.width);
        next ^= next >> self.width;
        self.value = (next & bit_mask(self.width) as u32) as u16;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HistoryState {
    ghr: u128,
    index_folds: [Fold; TAGGED_TABLES],
    tag_folds: [Fold; TAGGED_TABLES],
    tag_folds_minus_one: [Fold; TAGGED_TABLES],
}

impl Default for HistoryState {
    fn default() -> Self {
        Self::from_ghr(0)
    }
}

impl HistoryState {
    fn from_ghr(ghr: u128) -> Self {
        let ghr = ghr & bit_mask(MAX_HISTORY);
        Self {
            ghr,
            index_folds: std::array::from_fn(|table| Fold::new(HISTORY_LENGTHS[table], 9, ghr)),
            tag_folds: std::array::from_fn(|table| {
                Fold::new(HISTORY_LENGTHS[table], TAG_WIDTHS[table], ghr)
            }),
            tag_folds_minus_one: std::array::from_fn(|table| {
                Fold::new(HISTORY_LENGTHS[table], TAG_WIDTHS[table] - 1, ghr)
            }),
        }
    }

    fn append(&mut self, taken: bool) {
        let old_history = self.ghr;
        for fold in &mut self.index_folds {
            fold.append(old_history, taken);
        }
        for fold in &mut self.tag_folds {
            fold.append(old_history, taken);
        }
        for fold in &mut self.tag_folds_minus_one {
            fold.append(old_history, taken);
        }
        self.ghr = ((old_history << 1) | u128::from(taken)) & bit_mask(MAX_HISTORY);
        self.assert_consistent();
    }

    fn assert_consistent(&self) {
        for table in 0..TAGGED_TABLES {
            assert_eq!(
                self.index_folds[table].value,
                Fold::full(self.ghr, HISTORY_LENGTHS[table], 9)
            );
            assert_eq!(
                self.tag_folds[table].value,
                Fold::full(self.ghr, HISTORY_LENGTHS[table], TAG_WIDTHS[table])
            );
            assert_eq!(
                self.tag_folds_minus_one[table].value,
                Fold::full(self.ghr, HISTORY_LENGTHS[table], TAG_WIDTHS[table] - 1)
            );
        }
    }

    fn hashes(&self, pc: u64) -> ([usize; TAGGED_TABLES], [u16; TAGGED_TABLES]) {
        // Halfword addressing preserves PC[1], which is required by RV64C.
        let halfword_pc = pc >> 1;
        let pc_fold_8 =
            (halfword_pc ^ (halfword_pc >> 8) ^ (halfword_pc >> 16) ^ ((halfword_pc >> 24) & 0x7f))
                & 0xff;
        let pc_fold_9 =
            (halfword_pc ^ (halfword_pc >> 9) ^ (halfword_pc >> 18) ^ ((halfword_pc >> 27) & 0x0f))
                & 0x1ff;
        let indices = std::array::from_fn(|table| {
            ((halfword_pc ^ u64::from(self.index_folds[table].value)) & (TAGGED_ENTRIES as u64 - 1))
                as usize
        });
        let tags = std::array::from_fn(|table| {
            let pc_fold = if TAG_WIDTHS[table] == 8 {
                pc_fold_8
            } else {
                pc_fold_9
            };
            let mixed = pc_fold
                ^ u64::from(self.tag_folds[table].value)
                ^ (u64::from(self.tag_folds_minus_one[table].value) << 1);
            (mixed & bit_mask(TAG_WIDTHS[table]) as u64) as u16
        });
        (indices, tags)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TaggedEntry {
    valid: bool,
    tag: u16,
    /// Signed three-bit counter, saturated to [-4, 3].
    counter: i8,
    /// Two-bit usefulness counter.
    useful: u8,
}

impl TaggedEntry {
    fn predicts_taken(self) -> bool {
        self.counter >= 0
    }

    fn is_weak(self) -> bool {
        matches!(self.counter, -1 | 0)
    }

    fn train(&mut self, taken: bool) {
        self.counter = saturating_signed(self.counter, taken, -4, 3);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PredictionSource {
    Base,
    Tagged(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TagePrediction {
    pc: u64,
    base_index: usize,
    indices: [usize; TAGGED_TABLES],
    tags: [u16; TAGGED_TABLES],
    hits: [bool; TAGGED_TABLES],
    /// Longest matching tagged table.
    longest_match: Option<usize>,
    /// Second-longest match, or base when no second tagged table hits.
    true_alternate: PredictionSource,
    longest_match_prediction: bool,
    true_alternate_prediction: bool,
    longest_match_was_weak: bool,
    selected: PredictionSource,
    taken: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TageResolution {
    allocated_table: Option<usize>,
}

#[derive(Clone, Debug)]
struct TageModel {
    base: [u8; BASE_ENTRIES],
    tagged: [[TaggedEntry; TAGGED_ENTRIES]; TAGGED_TABLES],
    /// x^2+x+1 sequence: 01 -> 11 -> 10 -> 01.
    allocation_lfsr: u8,
}

impl Default for TageModel {
    fn default() -> Self {
        Self {
            // 01 is weak not-taken.
            base: [1; BASE_ENTRIES],
            tagged: [[TaggedEntry::default(); TAGGED_ENTRIES]; TAGGED_TABLES],
            allocation_lfsr: 0b01,
        }
    }
}

impl TageModel {
    fn prediction_from_source(
        &self,
        source: PredictionSource,
        base_index: usize,
        indices: &[usize; TAGGED_TABLES],
    ) -> bool {
        match source {
            PredictionSource::Base => self.base[base_index] >= 2,
            PredictionSource::Tagged(table) => self.tagged[table][indices[table]].predicts_taken(),
        }
    }

    fn predict(&self, pc: u64, history: &HistoryState) -> TagePrediction {
        let halfword_pc = pc >> 1;
        let base_index = (halfword_pc & (BASE_ENTRIES as u64 - 1)) as usize;
        let (indices, tags) = history.hashes(pc);
        let hits = std::array::from_fn(|table| {
            let entry = self.tagged[table][indices[table]];
            entry.valid && entry.tag == tags[table]
        });

        let longest_match = (0..TAGGED_TABLES).rev().find(|&table| hits[table]);
        let true_alternate = longest_match
            .and_then(|provider| {
                (0..provider)
                    .rev()
                    .find(|&table| hits[table])
                    .map(PredictionSource::Tagged)
            })
            .unwrap_or(PredictionSource::Base);
        let longest_match_prediction = longest_match
            .map(|table| self.tagged[table][indices[table]].predicts_taken())
            .unwrap_or(self.base[base_index] >= 2);
        let longest_match_was_weak = longest_match
            .map(|table| self.tagged[table][indices[table]].is_weak())
            .unwrap_or(false);
        let true_alternate_prediction =
            self.prediction_from_source(true_alternate, base_index, &indices);
        let selected = match longest_match {
            None => PredictionSource::Base,
            Some(_) if longest_match_was_weak => true_alternate,
            Some(table) => PredictionSource::Tagged(table),
        };
        let taken = self.prediction_from_source(selected, base_index, &indices);

        TagePrediction {
            pc,
            base_index,
            indices,
            tags,
            hits,
            longest_match,
            true_alternate,
            longest_match_prediction,
            true_alternate_prediction,
            longest_match_was_weak,
            selected,
            taken,
        }
    }

    fn train_base(&mut self, index: usize, taken: bool) {
        self.base[index] = saturating_unsigned(self.base[index], taken, 3);
    }

    fn train_source(&mut self, prediction: &TagePrediction, source: PredictionSource, taken: bool) {
        match source {
            PredictionSource::Base => self.train_base(prediction.base_index, taken),
            PredictionSource::Tagged(table) => {
                let entry = &mut self.tagged[table][prediction.indices[table]];
                assert!(entry.valid && entry.tag == prediction.tags[table]);
                entry.train(taken);
            }
        }
    }

    fn resolve(&mut self, prediction: &TagePrediction, taken: bool) -> TageResolution {
        if let Some(provider) = prediction.longest_match {
            let provider_source = PredictionSource::Tagged(provider);
            self.train_source(prediction, provider_source, taken);

            // A wrong weak provider also trains the prediction-time true
            // alternate, without ever training the provider twice.
            if prediction.longest_match_was_weak
                && prediction.longest_match_prediction != taken
                && prediction.true_alternate != provider_source
            {
                self.train_source(prediction, prediction.true_alternate, taken);
            }

            if prediction.longest_match_prediction != prediction.true_alternate_prediction {
                let entry = &mut self.tagged[provider][prediction.indices[provider]];
                entry.useful = saturating_unsigned(
                    entry.useful,
                    prediction.longest_match_prediction == taken,
                    3,
                );
            }
        } else {
            self.train_base(prediction.base_index, taken);
        }

        let mut allocated_table = None;
        let first_longer = prediction.longest_match.map_or(0, |table| table + 1);
        let allocation_inhibited =
            prediction.longest_match_was_weak && prediction.longest_match_prediction == taken;
        let allocation_attempt =
            prediction.taken != taken && first_longer < TAGGED_TABLES && !allocation_inhibited;
        if allocation_attempt {
            let scan_order = match self.allocation_lfsr {
                0b10 => [1, 2, 3, 0],
                0b11 => [2, 3, 0, 1],
                _ => [0, 1, 2, 3],
            };
            // The LFSR selects exactly one longer bank independently of its
            // free bit. A replaceable entry allocates immediately; a busy one
            // loses one usefulness count and is retried on a future miss.
            // This standard probabilistic variant avoids a cross-bank free-bit
            // priority recurrence in the one-update-per-clock FPGA pipeline.
            let table = scan_order
                .into_iter()
                .find(|table| *table >= first_longer)
                .unwrap();
            let entry = &mut self.tagged[table][prediction.indices[table]];
            if !entry.valid || entry.useful == 0 {
                *entry = TaggedEntry {
                    valid: true,
                    tag: prediction.tags[table],
                    counter: if taken { 0 } else { -1 },
                    useful: 0,
                };
                allocated_table = Some(table);
            } else {
                entry.useful = entry.useful.saturating_sub(1);
            }

            self.allocation_lfsr = match self.allocation_lfsr {
                0b01 => 0b11,
                0b11 => 0b10,
                _ => 0b01,
            };
        }

        TageResolution { allocated_table }
    }

    fn install(&mut self, table: usize, pc: u64, history: &HistoryState, counter: i8, useful: u8) {
        assert!((-4..=3).contains(&counter));
        assert!(useful <= 3);
        let (indices, tags) = history.hashes(pc);
        self.tagged[table][indices[table]] = TaggedEntry {
            valid: true,
            tag: tags[table],
            counter,
            useful,
        };
    }

    fn valid_entries(&self) -> usize {
        self.tagged
            .iter()
            .flat_map(|table| table.iter())
            .filter(|entry| entry.valid)
            .count()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BtbKind {
    Conditional,
    Direct,
    Indirect,
    Call,
    Return,
    Coroutine,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BtbEntry {
    valid: bool,
    pc: u64,
    target: u64,
    kind: Option<BtbKind>,
}

#[derive(Clone, Debug)]
struct Btb {
    entries: [[BtbEntry; BTB_WAYS]; BTB_SETS],
    /// Way selected for the next replacement in each set.
    lru: [usize; BTB_SETS],
}

impl Default for Btb {
    fn default() -> Self {
        Self {
            entries: [[BtbEntry::default(); BTB_WAYS]; BTB_SETS],
            lru: [0; BTB_SETS],
        }
    }
}

impl Btb {
    fn set(pc: u64) -> usize {
        // PC is halfword aligned, so PC[1] participates in the set index.
        ((pc >> 1) as usize) & (BTB_SETS - 1)
    }

    fn peek(&self, pc: u64) -> Option<BtbEntry> {
        self.entries[Self::set(pc)]
            .iter()
            .copied()
            .find(|entry| entry.valid && entry.pc == pc)
    }

    fn lookup(&mut self, pc: u64) -> Option<BtbEntry> {
        let set = Self::set(pc);
        let hit = self.entries[set]
            .iter()
            .position(|entry| entry.valid && entry.pc == pc)?;
        self.lru[set] = 1 - hit;
        Some(self.entries[set][hit])
    }

    fn update(&mut self, pc: u64, target: u64, kind: BtbKind) {
        let set = Self::set(pc);
        let way = self.entries[set]
            .iter()
            .position(|entry| entry.valid && entry.pc == pc)
            .or_else(|| self.entries[set].iter().position(|entry| !entry.valid))
            .unwrap_or(self.lru[set]);
        self.entries[set][way] = BtbEntry {
            valid: true,
            pc,
            target,
            kind: Some(kind),
        };
        self.lru[set] = 1 - way;
    }

    fn invalidate(&mut self, pc: u64) {
        let set = Self::set(pc);
        if let Some(way) = self.entries[set]
            .iter()
            .position(|entry| entry.valid && entry.pc == pc)
        {
            self.entries[set][way] = BtbEntry::default();
            self.lru[set] = way;
        }
    }

    fn clear_set(&mut self, set: usize) {
        self.entries[set] = [BtbEntry::default(); BTB_WAYS];
        self.lru[set] = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RasAction {
    None,
    Push,
    Pop,
    PopPush,
}

fn is_link_register(register: u8) -> bool {
    matches!(register, 1 | 5)
}

/// RISC-V's implicit RAS hint table for JAL/JALR and link registers x1/x5.
fn ras_action(is_jalr: bool, rd: u8, rs1: u8) -> RasAction {
    if !is_jalr {
        return if is_link_register(rd) {
            RasAction::Push
        } else {
            RasAction::None
        };
    }

    match (is_link_register(rd), is_link_register(rs1)) {
        (false, false) => RasAction::None,
        (false, true) => RasAction::Pop,
        (true, false) => RasAction::Push,
        (true, true) if rd == rs1 => RasAction::Push,
        (true, true) => RasAction::PopPush,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ras {
    entries: [u64; RAS_DEPTH],
    depth: usize,
}

impl Default for Ras {
    fn default() -> Self {
        Self {
            entries: [0; RAS_DEPTH],
            depth: 0,
        }
    }
}

impl Ras {
    fn push(&mut self, address: u64) {
        if self.depth == RAS_DEPTH {
            self.entries.rotate_left(1);
            self.entries[RAS_DEPTH - 1] = address;
        } else {
            self.entries[self.depth] = address;
            self.depth += 1;
        }
    }

    fn pop(&mut self) -> Option<u64> {
        if self.depth == 0 {
            None
        } else {
            self.depth -= 1;
            Some(self.entries[self.depth])
        }
    }

    fn top(&self) -> Option<u64> {
        self.depth.checked_sub(1).map(|index| self.entries[index])
    }

    /// Returns the popped prediction, if the action consumes a return address.
    fn apply(&mut self, action: RasAction, return_address: u64) -> Option<u64> {
        match action {
            RasAction::None => None,
            RasAction::Push => {
                self.push(return_address);
                None
            }
            RasAction::Pop => self.pop(),
            RasAction::PopPush => {
                let popped = self.pop();
                self.push(return_address);
                popped
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Checkpoint {
    token: u64,
    pc: u64,
    history_before: HistoryState,
    ras_before: Ras,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrontendPrediction {
    token: Option<u64>,
    history_before: HistoryState,
    btb_hit: bool,
    kind: Option<BtbKind>,
    tage: Option<TagePrediction>,
    taken: bool,
    next_pc: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActualControl {
    NonControl,
    Conditional { taken: bool, target: u64 },
    Direct { target: u64 },
    Indirect { target: u64 },
    Call { target: u64, return_address: u64 },
    Return { target: u64 },
    Coroutine { target: u64, return_address: u64 },
}

#[derive(Clone, Debug, Default)]
struct PredictorModel {
    tage: TageModel,
    btb: Btb,
    ras: Ras,
    history: HistoryState,
    checkpoints: Vec<Checkpoint>,
    next_token: u64,
    scrub_cursor: usize,
}

impl PredictorModel {
    fn ready(&self) -> bool {
        self.scrub_cursor == COLD_SCRUB_CYCLES
    }

    fn scrub_tick(&mut self) {
        if self.ready() {
            return;
        }
        let address = self.scrub_cursor;
        self.tage.base[address] = 1;
        if address < TAGGED_ENTRIES {
            for table in &mut self.tage.tagged {
                table[address] = TaggedEntry::default();
            }
        }
        if address < BTB_SETS {
            self.btb.clear_set(address);
        }
        if address == 0 {
            self.tage.allocation_lfsr = 0b01;
            self.ras = Ras::default();
            self.history = HistoryState::default();
            self.checkpoints.clear();
        }
        self.scrub_cursor += 1;
    }

    fn make_ready(&mut self) {
        while !self.ready() {
            self.scrub_tick();
        }
    }

    fn fetch(
        &mut self,
        pc: u64,
        instruction_length: u64,
        is_conditional: bool,
    ) -> FrontendPrediction {
        assert!(matches!(instruction_length, 2 | 4));
        let fallthrough = pc + instruction_length;
        if !self.ready() {
            return FrontendPrediction {
                token: None,
                history_before: self.history,
                btb_hit: false,
                kind: None,
                tage: None,
                taken: false,
                next_pc: fallthrough,
            };
        }

        let token = self.next_token;
        self.next_token += 1;
        let history_before = self.history;
        self.checkpoints.push(Checkpoint {
            token,
            pc,
            history_before,
            ras_before: self.ras,
        });

        let Some(entry) = self.btb.lookup(pc) else {
            let response = FrontendPrediction {
                token: Some(token),
                history_before,
                btb_hit: false,
                kind: None,
                tage: None,
                taken: false,
                next_pc: fallthrough,
            };
            if is_conditional {
                self.history.append(response.taken);
            }
            return response;
        };

        let kind = entry.kind.expect("valid BTB entries have a kind");
        let response = match kind {
            BtbKind::Conditional => {
                let prediction = self.tage.predict(pc, &self.history);
                FrontendPrediction {
                    token: Some(token),
                    history_before,
                    btb_hit: true,
                    kind: Some(kind),
                    tage: Some(prediction),
                    taken: prediction.taken,
                    next_pc: if prediction.taken {
                        entry.target
                    } else {
                        fallthrough
                    },
                }
            }
            BtbKind::Call => {
                self.ras.push(fallthrough);
                FrontendPrediction {
                    token: Some(token),
                    history_before,
                    btb_hit: true,
                    kind: Some(kind),
                    tage: None,
                    taken: true,
                    next_pc: entry.target,
                }
            }
            BtbKind::Return => FrontendPrediction {
                token: Some(token),
                history_before,
                btb_hit: true,
                kind: Some(kind),
                tage: None,
                taken: true,
                next_pc: self.ras.pop().unwrap_or(entry.target),
            },
            BtbKind::Coroutine => {
                let next_pc = self.ras.pop().unwrap_or(entry.target);
                self.ras.push(fallthrough);
                FrontendPrediction {
                    token: Some(token),
                    history_before,
                    btb_hit: true,
                    kind: Some(kind),
                    tage: None,
                    taken: true,
                    next_pc,
                }
            }
            BtbKind::Direct | BtbKind::Indirect => FrontendPrediction {
                token: Some(token),
                history_before,
                btb_hit: true,
                kind: Some(kind),
                tage: None,
                taken: true,
                next_pc: entry.target,
            },
        };
        if is_conditional {
            self.history.append(response.taken);
        }
        response
    }

    /// Models the externally visible result of the predictor's same-edge
    /// fallthrough lookup.  A taken predecessor cancels that speculative
    /// lookup before it can allocate a token or update history.
    fn fetch_fallthrough_shadow(
        &mut self,
        predecessor: FrontendPrediction,
        pc: u64,
        instruction_length: u64,
        is_conditional: bool,
    ) -> Option<FrontendPrediction> {
        if predecessor.taken {
            None
        } else {
            Some(self.fetch(pc, instruction_length, is_conditional))
        }
    }

    /// Restores the pre-prediction state, applies the actual control action,
    /// and invalidates the resolved checkpoint plus every younger checkpoint.
    fn repair(&mut self, token: u64, actual: ActualControl) -> usize {
        let position = self
            .checkpoints
            .iter()
            .position(|checkpoint| checkpoint.token == token)
            .expect("checkpoint token must still be live");
        let checkpoint = self.checkpoints[position];
        let younger = self.checkpoints.len() - position - 1;
        self.history = checkpoint.history_before;
        self.ras = checkpoint.ras_before;
        self.checkpoints.truncate(position);

        match actual {
            ActualControl::NonControl => self.btb.invalidate(checkpoint.pc),
            ActualControl::Conditional { taken, target } => {
                self.history.append(taken);
                self.btb.update(checkpoint.pc, target, BtbKind::Conditional);
            }
            ActualControl::Direct { target } => {
                self.btb.update(checkpoint.pc, target, BtbKind::Direct);
            }
            ActualControl::Indirect { target } => {
                self.btb.update(checkpoint.pc, target, BtbKind::Indirect);
            }
            ActualControl::Call {
                target,
                return_address,
            } => {
                self.ras.push(return_address);
                self.btb.update(checkpoint.pc, target, BtbKind::Call);
            }
            ActualControl::Return { target } => {
                self.ras.pop();
                self.btb.update(checkpoint.pc, target, BtbKind::Return);
            }
            ActualControl::Coroutine {
                target,
                return_address,
            } => {
                self.ras.pop();
                self.ras.push(return_address);
                self.btb.update(checkpoint.pc, target, BtbKind::Coroutine);
            }
        }
        self.history.assert_consistent();
        younger
    }

    fn has_checkpoint(&self, token: u64) -> bool {
        self.checkpoints
            .iter()
            .any(|checkpoint| checkpoint.token == token)
    }
}

#[test]
fn incremental_folds_equal_full_recomputation_for_every_geometry() {
    let widths = [7, 8, 9];
    let mut folds: Vec<Fold> = HISTORY_LENGTHS
        .iter()
        .flat_map(|&length| widths.iter().map(move |&width| Fold::new(length, width, 0)))
        .collect();
    let mut history = 0_u128;
    let mut random = 0xd1b5_4a32_d192_ed03_u64;

    for bit_number in 0..20_000 {
        // Fixed xorshift sequence: deterministic, but much less regular than an
        // alternating pattern and long enough to evict every history bit often.
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let new_bit = (random & 1) != 0;
        let old_history = history;
        for fold in &mut folds {
            fold.append(old_history, new_bit);
        }
        history = ((history << 1) | u128::from(new_bit)) & bit_mask(MAX_HISTORY);

        for fold in &folds {
            assert_eq!(
                fold.value,
                Fold::full(history, fold.history_length, fold.width),
                "fold mismatch at input bit {bit_number}, L={}, C={}",
                fold.history_length,
                fold.width
            );
        }
    }
}

#[test]
fn base_counter_saturates_and_uses_the_two_bit_threshold() {
    let mut tage = TageModel::default();
    let state = HistoryState::default();
    let pc = 0x4000;
    let index = ((pc >> 1) & (BASE_ENTRIES as u64 - 1)) as usize;

    assert!(!tage.predict(pc, &state).taken);
    for _ in 0..8 {
        tage.train_base(index, true);
    }
    assert_eq!(tage.base[index], 3);
    assert!(tage.predict(pc, &state).taken);
    for _ in 0..8 {
        tage.train_base(index, false);
    }
    assert_eq!(tage.base[index], 0);
    assert!(!tage.predict(pc, &state).taken);
}

#[test]
fn tagged_mispredict_allocates_at_most_one_and_learns_a_provider() {
    let mut tage = TageModel::default();
    let state = HistoryState::default();
    let pc = 0x8122;
    let base_index = ((pc >> 1) & (BASE_ENTRIES as u64 - 1)) as usize;
    tage.base[base_index] = 0;

    let cold = tage.predict(pc, &state);
    assert!(!cold.taken);
    let first = tage.resolve(&cold, true);
    assert_eq!(first.allocated_table, Some(0));
    assert_eq!(tage.valid_entries(), 1);

    // The newly allocated weak provider initially loses to the base alternate.
    // Resolve it once more to strengthen the tagged counter without allocating
    // another entry; the provider is selected after it leaves weak state.
    let weak_provider = tage.predict(pc, &state);
    assert_eq!(weak_provider.longest_match, Some(0));
    assert_eq!(weak_provider.selected, PredictionSource::Base);
    assert!(!weak_provider.taken);
    let before = tage.valid_entries();
    let second = tage.resolve(&weak_provider, true);
    assert_eq!(second.allocated_table, None);
    assert_eq!(tage.valid_entries(), before);

    let learned = tage.predict(pc, &state);
    assert_eq!(learned.longest_match, Some(0));
    assert_eq!(learned.selected, PredictionSource::Tagged(0));
    assert!(learned.taken);
    assert_eq!(tage.tagged[0][learned.indices[0]].counter, 1);
}

#[test]
fn correct_weak_lm_inhibits_allocation_when_true_alternate_was_wrong() {
    let state = HistoryState::from_ghr(0x6d);
    let pc = 0x9232;
    let mut tage = TageModel::default();
    let base_index = ((pc >> 1) & (BASE_ENTRIES as u64 - 1)) as usize;
    tage.base[base_index] = 0; // True alternate is not-taken.
    tage.install(0, pc, &state, 0, 0); // Weak LM is taken and is correct.

    let prediction = tage.predict(pc, &state);
    assert_eq!(prediction.longest_match, Some(0));
    assert!(prediction.longest_match_prediction);
    assert!(!prediction.taken);
    let valid_before = tage.valid_entries();
    let lfsr_before = tage.allocation_lfsr;

    let resolution = tage.resolve(&prediction, true);
    assert_eq!(resolution.allocated_table, None);
    assert_eq!(tage.valid_entries(), valid_before);
    assert_eq!(tage.allocation_lfsr, lfsr_before);
    assert_eq!(tage.tagged[0][prediction.indices[0]].counter, 1);
}

#[test]
fn disagreeing_wrong_provider_loses_usefulness() {
    let state = HistoryState::from_ghr(0x51);
    let pc = 0x9a32;
    let mut tage = TageModel::default();
    let base_index = ((pc >> 1) & (BASE_ENTRIES as u64 - 1)) as usize;
    tage.base[base_index] = 3; // True alternate predicts taken.
    tage.install(0, pc, &state, -2, 2); // Strong provider predicts not-taken.

    let prediction = tage.predict(pc, &state);
    assert_eq!(prediction.longest_match, Some(0));
    assert!(!prediction.longest_match_prediction);
    assert!(prediction.true_alternate_prediction);
    tage.resolve(&prediction, true);

    let entry = tage.tagged[0][prediction.indices[0]];
    assert_eq!(entry.counter, -1);
    assert_eq!(entry.useful, 1);
}

#[test]
fn full_pc_fold_distinguishes_equal_low_pc_and_history() {
    let state = HistoryState::from_ghr(0x2a55);
    let low_pc = 0x0000_0100;
    let high_pc = 0x0001_0100;
    let (low_indices, low_tags) = state.hashes(low_pc);
    let (high_indices, high_tags) = state.hashes(high_pc);

    // Indexing intentionally stays local, but every tag width sees PC[16].
    assert_eq!(low_indices, high_indices);
    for table in 0..TAGGED_TABLES {
        assert_ne!(low_tags[table], high_tags[table]);
    }
}

#[test]
fn allocation_bank_follows_the_rtl_lfsr_scan_order() {
    let state = HistoryState::default();
    let mut tage = TageModel::default();
    let mut allocated = Vec::new();

    for pc in [0xa000, 0xa002, 0xa004] {
        let base_index = ((pc >> 1) & (BASE_ENTRIES as u64 - 1)) as usize;
        tage.base[base_index] = 3;
        let prediction = tage.predict(pc, &state);
        assert!(prediction.taken);
        allocated.push(tage.resolve(&prediction, false).allocated_table.unwrap());
    }

    // LFSR 01/11/10 starts scans at T0/T2/T1 respectively.
    assert_eq!(allocated, [0, 2, 1]);
    assert_eq!(tage.allocation_lfsr, 0b01);
}

#[test]
fn busy_single_probe_decays_only_the_chosen_bank_without_fallback() {
    let state = HistoryState::from_ghr(0x155);
    let pc = 0xa800;
    let mut tage = TageModel::default();
    tage.install(0, pc, &state, -2, 1); // Strong NT provider.
    let prediction = tage.predict(pc, &state);
    assert_eq!(prediction.longest_match, Some(0));

    // LFSR=01 selects the first eligible longer component, T1. Occupy that
    // physical row with a different tag while leaving later T2 replaceable.
    let busy_tag = prediction.tags[1] ^ 1;
    tage.tagged[1][prediction.indices[1]] = TaggedEntry {
        valid: true,
        tag: busy_tag,
        counter: 3,
        useful: 2,
    };
    assert!(!tage.tagged[2][prediction.indices[2]].valid);

    let resolution = tage.resolve(&prediction, true);
    assert_eq!(resolution.allocated_table, None);
    assert_eq!(tage.tagged[1][prediction.indices[1]].tag, busy_tag);
    assert_eq!(tage.tagged[1][prediction.indices[1]].counter, 3);
    assert_eq!(tage.tagged[1][prediction.indices[1]].useful, 1);
    assert!(!tage.tagged[2][prediction.indices[2]].valid);
}

#[test]
fn weak_longest_match_selects_true_alternate() {
    let state = HistoryState::from_ghr(0x000a_553c);
    let pc = 0x1036;
    let mut tage = TageModel::default();
    tage.install(0, pc, &state, 3, 1); // strong taken true alternate
    tage.install(1, pc, &state, -1, 0); // weak NT, longest match

    let prediction = tage.predict(pc, &state);
    assert_eq!(prediction.longest_match, Some(1));
    assert_eq!(prediction.true_alternate, PredictionSource::Tagged(0));
    assert_eq!(prediction.selected, PredictionSource::Tagged(0));
    assert!(prediction.taken);
}

#[test]
fn exact_conditional_instruction_classifier_rejects_reserved_encodings() {
    for funct3 in 0_u32..8 {
        let standard_branch = (funct3 << 12) | 0b1100011;
        assert_eq!(
            conditional_instruction_hint(standard_branch, false),
            !matches!(funct3, 0b010 | 0b011),
            "standard branch funct3={funct3:03b}"
        );
    }
    assert!(!conditional_instruction_hint(0b0010011, false));

    for quadrant in 0_u32..4 {
        for funct3 in 0_u32..8 {
            let compressed = (funct3 << 13) | quadrant;
            assert_eq!(
                conditional_instruction_hint(compressed, true),
                quadrant == 0b01 && matches!(funct3, 0b110 | 0b111),
                "compressed quadrant={quadrant:02b}, funct3={funct3:03b}"
            );
        }
    }

    // The fetch length/classification bit is part of the decode contract.
    assert!(!conditional_instruction_hint(0b1100011, true));
    assert!(!conditional_instruction_hint(0b110 << 13 | 0b01, false));
}

#[test]
fn cold_btb_miss_conditional_speculates_not_taken_then_repairs_actual() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    predictor.history = HistoryState::from_ghr(0x35);
    let before = predictor.history;

    let miss = predictor.fetch(0x2000, 4, true);
    assert!(!miss.btb_hit);
    let mut expected_not_taken = before;
    expected_not_taken.append(false);
    assert_eq!(predictor.history, expected_not_taken);
    let invalidated = predictor.repair(
        miss.token.unwrap(),
        ActualControl::Conditional {
            taken: true,
            target: 0x2100,
        },
    );

    let mut expected = before;
    expected.append(true);
    assert_eq!(predictor.history, expected);
    assert_eq!(invalidated, 0);
    assert_eq!(
        predictor.btb.peek(0x2000).unwrap().kind,
        Some(BtbKind::Conditional)
    );
}

#[test]
fn false_btb_hit_restores_history_and_discards_younger_state() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    predictor.history = HistoryState::from_ghr(0x2d);
    let pc = 0x3002;
    predictor.btb.update(pc, 0x3800, BtbKind::Conditional);
    let base_index = ((pc >> 1) & (BASE_ENTRIES as u64 - 1)) as usize;
    predictor.tage.base[base_index] = 3;
    let before = predictor.history;

    let false_hit = predictor.fetch(pc, 2, false);
    assert!(false_hit.taken);
    assert_eq!(predictor.history, before);
    let younger = predictor.fetch(0x4000, 4, false);
    assert!(predictor.has_checkpoint(younger.token.unwrap()));

    assert_eq!(
        predictor.repair(false_hit.token.unwrap(), ActualControl::NonControl),
        1
    );
    assert_eq!(predictor.history, before);
    assert!(predictor.btb.peek(pc).is_none());
    assert!(!predictor.has_checkpoint(false_hit.token.unwrap()));
    assert!(!predictor.has_checkpoint(younger.token.unwrap()));
}

#[test]
fn rollback_keeps_older_checkpoint_and_removes_all_younger_ones() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    for (pc, target) in [(0x5000, 0x5100), (0x5002, 0x5200), (0x5004, 0x5300)] {
        predictor.btb.update(pc, target, BtbKind::Conditional);
    }
    predictor.tage.base[((0x5000_u64 >> 1) & 0x7ff) as usize] = 3;
    predictor.tage.base[((0x5002_u64 >> 1) & 0x7ff) as usize] = 0;
    predictor.tage.base[((0x5004_u64 >> 1) & 0x7ff) as usize] = 3;

    let older = predictor.fetch(0x5000, 2, true);
    let resolved = predictor.fetch(0x5002, 2, true);
    let younger = predictor.fetch(0x5004, 2, true);
    let actual = !resolved.taken;
    let mut expected = resolved.history_before;
    expected.append(actual);

    assert_eq!(
        predictor.repair(
            resolved.token.unwrap(),
            ActualControl::Conditional {
                taken: actual,
                target: 0x5200,
            },
        ),
        1
    );
    assert_eq!(predictor.history, expected);
    assert!(predictor.has_checkpoint(older.token.unwrap()));
    assert!(!predictor.has_checkpoint(resolved.token.unwrap()));
    assert!(!predictor.has_checkpoint(younger.token.unwrap()));
}

#[test]
fn two_way_btb_uses_halfword_pc_and_true_lru_replacement() {
    let mut btb = Btb::default();
    let a = 0x6000;
    let b = a + (BTB_SETS as u64 * 2);
    let c = b + (BTB_SETS as u64 * 2);
    assert_eq!(Btb::set(a), Btb::set(b));
    assert_eq!(Btb::set(b), Btb::set(c));

    btb.update(a, 0x7000, BtbKind::Direct);
    btb.update(b, 0x7100, BtbKind::Indirect);
    assert!(btb.peek(a).is_some());
    assert!(btb.peek(b).is_some());

    // Touch A, making B the least-recently-used way, then collide C.
    assert_eq!(btb.lookup(a).unwrap().target, 0x7000);
    btb.update(c, 0x7200, BtbKind::Conditional);
    assert!(btb.peek(a).is_some());
    assert!(btb.peek(b).is_none());
    assert!(btb.peek(c).is_some());
}

#[test]
fn ras_obeys_x1_x5_call_return_and_coroutine_hints() {
    assert_eq!(ras_action(false, 1, 0), RasAction::Push);
    assert_eq!(ras_action(false, 5, 0), RasAction::Push);
    assert_eq!(ras_action(false, 0, 0), RasAction::None);
    assert_eq!(ras_action(true, 0, 1), RasAction::Pop);
    assert_eq!(ras_action(true, 0, 5), RasAction::Pop);
    assert_eq!(ras_action(true, 1, 2), RasAction::Push);
    assert_eq!(ras_action(true, 1, 1), RasAction::Push);
    assert_eq!(ras_action(true, 5, 1), RasAction::PopPush);
    assert_eq!(ras_action(true, 1, 5), RasAction::PopPush);

    let mut ras = Ras::default();
    let ordinary_return = 0x8000 + 4;
    assert_eq!(ras.apply(RasAction::Push, ordinary_return), None);
    assert_eq!(ras.top(), Some(ordinary_return));
    assert_eq!(ras.apply(RasAction::Pop, 0), Some(ordinary_return));

    ras.push(0x9004);
    assert_eq!(ras.apply(ras_action(true, 5, 1), 0xa004), Some(0x9004));
    assert_eq!(ras.top(), Some(0xa004));

    // A compressed call links to PC+2, not PC+4.
    let compressed_pc = 0xb002;
    ras.apply(RasAction::Push, compressed_pc + 2);
    assert_eq!(ras.top(), Some(0xb004));
}

#[test]
fn indirect_repair_installs_the_resolved_jalr_target() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    let pc = 0xb402;
    let target = 0xc180;

    let cold = predictor.fetch(pc, 2, false);
    assert!(!cold.btb_hit);
    predictor.repair(cold.token.unwrap(), ActualControl::Indirect { target });

    let entry = predictor.btb.peek(pc).unwrap();
    assert_eq!(entry.kind, Some(BtbKind::Indirect));
    assert_eq!(entry.target, target);
    let warm = predictor.fetch(pc, 2, false);
    assert_eq!(warm.kind, Some(BtbKind::Indirect));
    assert!(warm.taken);
    assert_eq!(warm.next_pc, target);
}

#[test]
fn return_repair_keeps_the_real_target_as_the_empty_ras_fallback() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    let pc = 0xb802;
    let target = 0xa404;
    predictor.ras.push(target);

    let cold = predictor.fetch(pc, 2, false);
    assert!(!cold.btb_hit);
    predictor.repair(cold.token.unwrap(), ActualControl::Return { target });
    assert_eq!(predictor.ras.top(), None);

    let entry = predictor.btb.peek(pc).unwrap();
    assert_eq!(entry.kind, Some(BtbKind::Return));
    assert_eq!(entry.target, target);
    let warm = predictor.fetch(pc, 2, false);
    assert_eq!(warm.kind, Some(BtbKind::Return));
    assert_eq!(warm.next_pc, target);
    assert_eq!(predictor.ras.top(), None);
}

#[test]
fn coroutine_repair_and_prediction_both_pop_then_push() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    let pc = 0xbc02;
    let old_link = 0xa804;
    let repaired_link = 0xac04;
    predictor.ras.push(old_link);

    let cold = predictor.fetch(pc, 2, false);
    assert!(!cold.btb_hit);
    predictor.repair(
        cold.token.unwrap(),
        ActualControl::Coroutine {
            target: old_link,
            return_address: repaired_link,
        },
    );
    assert_eq!(predictor.ras.top(), Some(repaired_link));

    let entry = predictor.btb.peek(pc).unwrap();
    assert_eq!(entry.kind, Some(BtbKind::Coroutine));
    assert_eq!(entry.target, old_link);

    let mut empty_ras = predictor.clone();
    empty_ras.ras = Ras::default();
    let fallback = empty_ras.fetch(pc, 2, false);
    assert_eq!(fallback.kind, Some(BtbKind::Coroutine));
    assert_eq!(fallback.next_pc, old_link);
    assert_eq!(empty_ras.ras.top(), Some(pc + 2));

    let warm = predictor.fetch(pc, 2, false);
    assert_eq!(warm.kind, Some(BtbKind::Coroutine));
    assert_eq!(warm.next_pc, repaired_link);
    assert_eq!(predictor.ras.top(), Some(pc + 2));
}

#[test]
fn continuous_not_taken_b_hash_includes_the_speculative_a_prediction() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    predictor.history = HistoryState::from_ghr(0x155);
    let a_pc = 0xc002;
    let b_pc = 0xc004;
    predictor.btb.update(a_pc, 0xd000, BtbKind::Conditional);
    predictor.btb.update(b_pc, 0xd100, BtbKind::Conditional);

    let a = predictor.fetch(a_pc, 2, true);
    assert!(!a.taken);
    let b = predictor
        .fetch_fallthrough_shadow(a, b_pc, 2, true)
        .expect("not-taken fallthrough remains visible");
    let b_tage = b.tage.unwrap();

    let mut expected_history = a.history_before;
    expected_history.append(a.taken);
    let expected_hash = expected_history.hashes(b_pc);
    let stale_hash = a.history_before.hashes(b_pc);
    assert_eq!(b_tage.indices, expected_hash.0);
    assert_eq!(b_tage.tags, expected_hash.1);
    assert_ne!(
        (b_tage.indices, b_tage.tags),
        stale_hash,
        "B was hashed with history from before A"
    );
}

#[test]
fn taken_fallthrough_shadow_consumes_neither_token_nor_history() {
    let mut predictor = PredictorModel::default();
    predictor.make_ready();
    predictor.history = HistoryState::from_ghr(0x155);
    let a_pc = 0xc802;
    let fallthrough_pc = a_pc + 2;
    let target_pc = 0xd800;
    predictor.btb.update(a_pc, target_pc, BtbKind::Conditional);
    predictor.tage.base[((a_pc >> 1) & 0x7ff) as usize] = 3;

    let a = predictor.fetch(a_pc, 2, true);
    assert!(a.taken);
    let history_after_a = predictor.history;
    let token_after_a = predictor.next_token;

    let shadow = predictor.fetch_fallthrough_shadow(a, fallthrough_pc, 2, true);
    assert_eq!(shadow, None);
    assert_eq!(predictor.history, history_after_a);
    assert_eq!(predictor.next_token, token_after_a);

    let target = predictor.fetch(target_pc, 4, false);
    assert_eq!(target.token, Some(a.token.unwrap() + 1));
    assert_eq!(target.history_before, history_after_a);
    assert_eq!(predictor.history, history_after_a);
}

#[test]
fn cold_predictor_is_gated_until_every_uninitialized_array_is_scrubbed() {
    let mut predictor = PredictorModel::default();
    let pc = 0xe002;
    let base_index = ((pc >> 1) & 0x7ff) as usize;
    predictor.tage.base[base_index] = 3;
    predictor.tage.install(3, pc, &predictor.history, 3, 3);
    predictor.btb.update(pc, 0xf000, BtbKind::Conditional);

    let cold = predictor.fetch(pc, 2, true);
    assert!(!cold.btb_hit);
    assert!(!cold.taken);
    assert_eq!(cold.next_pc, pc + 2);
    assert_eq!(cold.token, None);

    for _ in 0..COLD_SCRUB_CYCLES - 1 {
        predictor.scrub_tick();
    }
    assert!(!predictor.ready());
    assert_eq!(predictor.fetch(pc, 2, true).token, None);
    predictor.scrub_tick();
    assert!(predictor.ready());
    assert_eq!(predictor.tage.base[base_index], 1);
    assert!(predictor.btb.peek(pc).is_none());
    assert_eq!(predictor.tage.valid_entries(), 0);

    predictor.tage.base[base_index] = 3;
    predictor.btb.update(pc, 0xf000, BtbKind::Conditional);
    let live = predictor.fetch(pc, 2, true);
    assert!(live.btb_hit);
    assert!(live.taken);
    assert_eq!(live.next_pc, 0xf000);
    assert!(live.token.is_some());
}

#[test]
fn back_to_back_same_index_training_composes_from_the_latest_counter() {
    let mut tage = TageModel::default();
    let history = HistoryState::default();
    let pc = 0x12_3402;

    // Both tokens observe the same weak-not-taken word before either resolves.
    let older = tage.predict(pc, &history);
    let younger = tage.predict(pc, &history);
    assert!(!older.taken);
    assert!(!younger.taken);
    assert_eq!(older.base_index, younger.base_index);

    tage.resolve(&older, true);
    tage.resolve(&younger, true);
    assert_eq!(tage.base[older.base_index], 3);

    // One subsequent base-table update must leave the composed 3 -> 2 state.
    // The preceding mispredicts also allocate tagged entries, so exercise B
    // directly rather than letting a new tagged provider redirect training.
    tage.train_base(older.base_index, false);
    assert_eq!(tage.base[older.base_index], 2);
    assert!(tage.predict(pc, &history).taken);
}

#[test]
fn interleaved_a_b_a_training_does_not_lose_a_counter_state() {
    let mut tage = TageModel::default();
    let history = HistoryState::default();
    let a_pc = 0x22_0202;
    let b_pc = 0x22_0c42;
    let older_a = tage.predict(a_pc, &history);
    let middle_b = tage.predict(b_pc, &history);
    let younger_a = tage.predict(a_pc, &history);
    assert_eq!(older_a.base_index, younger_a.base_index);
    assert_ne!(older_a.base_index, middle_b.base_index);

    tage.resolve(&older_a, true);
    tage.resolve(&middle_b, true);
    tage.resolve(&younger_a, true);
    assert_eq!(tage.base[older_a.base_index], 3);

    tage.train_base(older_a.base_index, false);
    assert_eq!(tage.base[older_a.base_index], 2);
    assert!(tage.predict(a_pc, &history).taken);
}
