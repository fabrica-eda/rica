//! Find legal, unoccupied detours of JTDI in an otherwise frozen bad design.
//! This is a physical diagnostic, not a timing qualification or a router fix.
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fs::File,
    io::BufReader,
};
use texo_model::{PipId, WireId};
use texo_target_ecp5::{Ecp5Architecture, read_architecture_cache};

fn path(arch: &Ecp5Architecture, busy: &[bool], start: WireId, goal: WireId) -> Option<Vec<PipId>> {
    let mut prev = HashMap::new();
    let mut queue = VecDeque::from([start]);
    prev.insert(start, (start, PipId(usize::MAX)));
    while let Some(from) = queue.pop_front() {
        if from == goal {
            break;
        }
        if prev.len() > 100_000 {
            return None;
        }
        for (to, pip) in arch.device().routing_neighbors(from).ok()? {
            if prev.contains_key(&to) || (busy[to.0] && to != goal) {
                continue;
            }
            let wire = &arch.device().wires()[to.0];
            let local = wire.name.rsplit('/').next()?;
            if to != goal
                && (!matches!(local.as_bytes()[0], b'H' | b'V')
                    || !(66..=84).contains(&wire.point.y)
                    || !(36..=55).contains(&wire.point.x))
            {
                continue;
            }
            let meta = arch.pip_metadata(pip);
            if meta.lutperm_flags != 0 {
                continue;
            }
            prev.insert(to, (from, pip));
            queue.push_back(to);
        }
    }
    let mut out = Vec::new();
    let mut node = goal;
    while node != start {
        let &(from, pip) = prev.get(&node)?;
        out.push(pip);
        node = from;
    }
    out.reverse();
    Some(out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 3, "ARCH CHECKPOINT OUTPUT");
    let arch = read_architecture_cache(BufReader::new(File::open(&args[0])?))?;
    eprintln!("architecture loaded");
    let cp: Value = serde_json::from_reader(BufReader::new(File::open(&args[1])?))?;
    let routes = cp["routes"].as_array().ok_or("routes")?;
    let old = routes
        .iter()
        .find(|r| r["net_id"] == 10)
        .ok_or("JTDI net")?;
    assert_eq!(old["driver_wire"], "R94C4/JJTDI_JTAG");
    let mut busy = vec![false; arch.device().wires().len()];
    for route in routes {
        for w in route["wires"].as_array().ok_or("wires")? {
            busy[w["wire_id"].as_u64().ok_or("wire id")? as usize] = true;
        }
    }
    let id = |name: &str| {
        arch.device()
            .wires()
            .iter()
            .position(|w| w.name == name)
            .map(WireId)
            .unwrap()
    };
    let start = id("R79C46/V06N0003");
    let goal = id("R74C45/M0");
    let mut tail = Vec::new();
    let mut cursor = start;
    loop {
        let edge = old["pips"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["from_wire_id"] == cursor.0)
            .ok_or("old path")?;
        tail.push(edge.clone());
        cursor = WireId(edge["to_wire_id"].as_u64().unwrap() as usize);
        busy[cursor.0] = false;
        if cursor == goal {
            break;
        }
    }
    let speed = &arch.speed_grades()["8_5G"];
    let encode = |pip: PipId| {
        let p = &arch.device().pips()[pip.0];
        let m = arch.pip_metadata(pip);
        let t = &speed.pip_classes[m.timing_class];
        json!({"pip_id":pip.0, "from":arch.device().wires()[p.from().0].name,
            "to":arch.device().wires()[p.to().0].name, "from_wire_id":p.from().0,
            "to_wire_id":p.to().0, "fixed":m.fixed, "config_tile":m.config_tile,
            "tile_type":m.tile_type, "timing_class":m.timing_class,
            "min_ps":t.base.min_ps+t.fanout_adder.min_ps,
            "max_ps":t.base.max_ps+t.fanout_adder.max_ps})
    };
    let old_min: u64 = tail
        .iter()
        .map(|p| {
            encode(PipId(p["pip_id"].as_u64().unwrap() as usize))["min_ps"]
                .as_u64()
                .unwrap()
        })
        .sum();
    let old_max: u64 = tail
        .iter()
        .map(|p| {
            encode(PipId(p["pip_id"].as_u64().unwrap() as usize))["max_ps"]
                .as_u64()
                .unwrap()
        })
        .sum();
    let mut results = Vec::new();
    for row in [70, 72, 74, 76, 78, 80] {
        for col in [40, 43, 46, 49, 52] {
            let name = format!("R{row}C{col}/H02E0001");
            let mid = id(&name);
            if busy[mid.0] {
                continue;
            }
            let Some(mut first) = path(&arch, &busy, start, mid) else {
                continue;
            };
            let mut blocked = busy.clone();
            for &p in &first {
                blocked[arch.device().pips()[p.0].to().0] = true;
            }
            let Some(second) = path(&arch, &blocked, mid, goal) else {
                continue;
            };
            first.extend(second);
            let pips: Vec<_> = first.into_iter().map(encode).collect();
            let min: u64 = pips.iter().map(|p| p["min_ps"].as_u64().unwrap()).sum();
            let max: u64 = pips.iter().map(|p| p["max_ps"].as_u64().unwrap()).sum();
            eprintln!(
                "{name}: {} pips, tail {min}/{max} ps (old {old_min}/{old_max})",
                pips.len()
            );
            results.push(json!({"waypoint":name,"tail_min_ps":min,"tail_max_ps":max,"pips":pips}));
        }
    }
    serde_json::to_writer_pretty(
        File::create(&args[2])?,
        &json!({
        "purpose":"diagnostic only; no complete timing qualification",
        "net_id":10,"start":"R79C46/V06N0003","goal":"R74C45/M0",
        "old_tail_min_ps":old_min,"old_tail_max_ps":old_max,"removed_pips":tail,
        "candidates":results}),
    )?;
    Ok(())
}
