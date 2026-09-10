# 自作OSを動かすためのRV64 ISA・プラットフォーム計画

2026-09-10。自作OSを前提に、整数命令の追加より特権・割り込み・メモリ保護を優先する。
ユーザーの選択により **M/S/U-mode＋Sv39による保護・仮想記憶を持つOS** を目標として確定した。
最初はM-mode基盤を完成させ、段階的にS-mode kernelとU-mode processへ進む。
現時点でS-modeやMMUが既に使えるとは扱わない。

## 現在地

認定済み基準は `HardwareCompactInterruptTestTop`。
CPU 124.8 MHz、メモリ62.4 MHz、setup・hold各200 ps予約、実機326実行合格。
タイマーで2タスクの全整数contextを16回切り替える試験も3回成功した。
既存35診断は同一ELF・同一命令数で±1 cycle以内、通常処理の性能を維持している。
[実測記録](../toolchain/measurements/2026-09-10-rv64-machine-interrupts/PERFORMANCE.md)。

| 分類 | 認定済み基準 | OS向けに必要な差分 |
| --- | --- | --- |
| 整数ISA | RV64 I/M/B、整数圧縮Zca、Zicsr、Zifencei | A（Zaamo＋Zalrsc）。F/D/Vは後工程 |
| 特権 | M-modeのみ。ECALL/EBREAK/不正命令、MRET | M/S/Uの実行権限、CSRアクセス権、例外委譲 |
| CSR | 従来のM-mode CSRに加え、割り込み構成ではmie/mipとmcause[63] | S-mode CSR、権限と委譲、time alias |
| 割り込み | MSI/MTI/MEI受付、MMIO timer/MSIP。MSI/MTIとプリエンプションは実機認定、MEIはCPU試験 | 外部デバイス/controllerの接続 |
| アドレス | PC・分岐先・EPC・mtvalは内部32 bit。メモリ要求も32 bit | 仮想アドレスを切り捨てず保持し、変換後に実装物理幅を検査 |
| メモリ | 256 KiB BRAM中252 KiB RAM＋4 KiB reset領域。範囲外は別名になる | RAM/MMIOの厳密なdecode、access fault、PMP、ページfault |
| 保護・仮想記憶 | PMP/MMU/TLBなし | U-mode隔離、S-mode、Sv39、SFENCE.VMA |
| I/O | 停止中のJTAG loaderと試験終了用観測 | UART、時刻源、IRQ制御、必要に応じ外部RAM |

根拠は `src/Core.veryl`、`CsrFile.veryl`、`Decoder.veryl`、
`PipelineInterfaces.veryl`、`HardwareMemory.veryl`。
特に現行の32-bit PC/EPCのまま `satp` を追加してもSv39にはならない。
BRAMの範囲外aliasも、保護があるOSのfault処理には使えない。

開発中の `HardwareCompactAccessFaultTestTop` は `STRICT_MEMORY=1` を追加し、
64-bit PC・分岐先・trap値と、RAM/MMIOの厳密な範囲検査を実装した。
この構成の配置配線・実機認定は別途行う。通常load/storeに待ち段は追加していない。
[Rust kernel](kernel/README.md) も実装し、認定済み基準で2タスクのyieldとタイマ切り替えを
3回実機確認した。範囲外load/store、64-bit fetch先、RAM末尾の命令境界から復帰する
kernel variantを、新しいCPU構成の試験に用いる。

アドレス検査と割り込み受理の更新は、CPU試験60件と238 ELF×2通りのclock順序で合格した。
Rust kernelは `python3 riscv/kernel/check.py` でCPUモデルと一緒に検証できる。
通常診断35本の命令数・サイクル数は基準と一致している。
その後、転送元ごとの範囲検査、メモリポート用アドレスの領域タグ再利用、
演算結果の選択条件の前段保持、実行結果レジスタのenableを整理した。
全workspace 242件、厳密構成476実行、無効構成468実行、Rust kernelの6実行に合格した。
JTAG経由のnative board試験では終了storeを含む命令数の凍結と、clear後の再実行も確認した。
さらにloadの幅・符号拡張を既存のCPU応答レジスタの手前へ移す試作を追加した。
この変更も238 ELF×2通りで合格し、通常診断35本とRust kernelのサイクル数を維持した。
続いて分岐予測先との比較を16 bitごとに保持し、CMで結果をまとめる構成を試作した。
CPU試験60件、厳密構成476実行、無効構成468実行に合格し、分岐回復とkernelのサイクル数を維持した。
除算の剰余ゼロ判定を保持し、アドレス末尾の境界判定から追加の加算器を除いた。
さらにZbaの加算前処理を選ぶ3 bitをRX→EXで保持した。最新の組合せはCPU・整数演算試験75件、
厳密構成476実行、無効構成468実行に合格し、通常診断35本とkernelのサイクル数を維持した。
末尾オフセットを13 bitで前段保持し、開始・末尾のページを並行検査する試作も同じ回帰に合格した。
境界判定のnative／mapped 706,848ケースに加え、CPUのfault試験を即値−2048／−1／0／2047へ広げた。
新構成の124.8 MHz STAはまだ未達で、認定済みimageの置き換えは行っていない。
[実装・検証と配置配線の記録](../toolchain/measurements/2026-09-10-rv64-access-faults/README.md)。

## 実装順と合格条件

### CPUとOSを同じ開発ループに置く

最小kernelは **Rust `no_std`＋起動・trap入口のアセンブリ** で実装する。
CPU機能の追加と、それを使うkernel経路・自己検査ELFを一組で進める。
M-mode固有のCSR・timer操作はアーキテクチャ層へ置き、後のS-mode移行に備える。
初版は固定メモリと少数タスクを用い、起動・例外記録・yield・タイマー切り替えを持つ。
Rustの`core`もAを含まないターゲットでビルドし、生成ELFを命令単位で検査する。

今回のCPU側はアドレス検査とaccess fault、kernel側は元のPC・原因・アドレスの記録、
意図的なfaultからの復帰、タスク継続を実装する。以降はAとatomic操作、U/PMPとsyscall・隔離、
S/Sv39とページ表・プロセス切り替えを対応させる。
頻繁な検証はモデルで回し、RTL更新時は124.8 MHzのSTA・checked bitgen・実機まで確認する。
OSだけの変更は認定済みbitstreamを再利用する。固定処理のIPCに加え、固定回数のyieldや
例外処理に要するサイクルも記録する。CPUとOSの共通の誤りを見逃さないよう、
仕様に基づく単体試験・ACT/SailはOSから独立して維持する。

### 0. 完了：除算改善とOS起動時のSYSTEM動作

除算早期終了を実装し、無効設定で従来MDUと比較した。
符号、ゼロ除算、overflow、W命令、cancel、未消費結果を検証し、同一ELFの実機時間を記録した。
固定 `misa` への全6種類のCSR書込みとM-modeの `WFI` を合法にした。
`WFI` は仕様で許されるNOP実装。省電力停止や割り込み実装の完成とは区別する。
実機のOS起動試験で、これらに加えてMSCRATCH stack交換、trap frame、ECALL→MRETを確認した。

### 1. M-modeでプリエンプションできる最小基盤

`ENABLE_INTERRUPTS=1` と `HardwareCompactInterruptTestTop` に割り込み・タイマー基盤を実装。
[CSR、受付境界、MMIO、resetの仕様](RV64_MACHINE_PLATFORM.md)。
2タスクの全整数contextを16回切り替える試験は、両方のシミュレーションclock順序、
同時edgeモデルと実機3回で合格した。124.8 MHz、setup・hold各200 ps予約を維持した。
[STA・実機の認定記録](../toolchain/measurements/2026-09-10-rv64-machine-interrupts/README.md)。
この工程の残りは厳密なメモリdecode/access faultと外部I/O。S/U/PMP/Sv39は後工程のまま。

- `mie/mip` のMSI/MTI/MEI、`mcause[63]`、MIE/MPIEの正しい更新。
  pendingはsourceに従うlevel入力とし、CSRで読める状態と割り込み判定を一致させる。
- commit境界で受付。既発行のstoreやMDUは完了境界を明示し、途中の副作用を再実行しない。
  分岐・同期例外・MRET・FENCE.Iとの優先順位を定義し、次に実行するPCを `mepc` に保存する。
- MMIO `mtime/mtimecmp` とsoftware interrupt、外部IRQ接続。時刻はOSへ周波数を公開する。
  CPU停止・debug resetとの関係、64-bit MMIOとbyte enableも仕様化する。
- `mtvec` はまずdirect mode。未実装vector modeへの書込みはWARLとしてdirectへ変換する。
- RAMとMMIOの境界を切り、範囲外fetch/load/storeをaccess faultへ送る。
  投機fetchがMMIOを読まないこと、faultしたstoreが書き込まないことを保証する。

合格条件は、2タスクの全整数レジスタを保存・復元する小さなOS試験がタイマで切り替わり、
MIEを外している間の保留、MRET直後の再受付、長いload/divide中、誤予測、例外競合を通ること。
UARTとRAMの配置・起動引数もここで短いプラットフォーム仕様として固定する。

次の実装単位はRAM/MMIOの厳密なアドレス検査とaccess faultとする。
RV64の有効アドレスを検査前に32 bitへ切り捨てず、上位bitによる別名も拒否する。
fetch/load/storeそれぞれで元アドレスを例外情報へ残し、faultしたstoreは一度も書き込まない。
境界を跨ぐaccess、誤予測で破棄されるfetch、割り込みとfaultの競合を負試験に含める。
この検査を確立してから外部I/O、A、下位特権・PMPへ進む。

### 2. A拡張：OSのlockとコンパイラのatomic

`LR.W/D`、`SC.W/D` と全AMO.W/Dを実装し、Zaamo＋Zalrscが揃ってから `misa.A` を立てる。
単一hartの初版はLSUで直列化し、`aq/rl` とFENCEの順序を維持する。
reservationは物理アドレスで追跡し、SCの成功/失敗、word結果の符号拡張、alignment fault、
途中trapとcontext switch、外部書込みによる無効化、constrained LR/SC loopの進捗を検証する。
MMIOのatomic可否はメモリ属性で判定する。

単一hartの最初のM-mode OSは割り込み禁止区間でも実装できるので、Aの完成をタイマ起動の前提にしない。
A対応後のソフトウェアターゲットは `rv64imac_zicsr_zifencei_zba_zbb_zbs`、ABIはまず `lp64`。
現行image用ビルドにA命令を混ぜない。

### 3. U-modeと保護、その後S-mode＋Sv39

1. privilege stateとCSRアクセス権、MRETの権限遷移、U-mode ECALL、PMP。
   ユーザコードがCSRやkernel RAM/MMIOへ触れる負試験も用意する。
2. S-mode CSR、SRET、medeleg/mideleg、counterアクセス制御。
   M-mode firmwareからS-mode kernelを起動し、U-mode syscallと例外復帰を試す。
3. `satp` のBare/Sv39、64-bit VAのcanonical判定、3段page walk、ITLB/DTLB、
   R/W/X/U/SUM/MXR、4 KiB/2 MiB/1 GiB page、fault時の元VA、SFENCE.VMA。
   PTEのA/D管理方式とASID幅は実装着手時に明示する。まずASID幅0も候補とする。

ページ境界を跨ぐ非整列accessも試す。storeは対象範囲すべての変換・権限検査が済む前に書き込まない。
TLBは小容量から始め、page walk中は依存要求を待たせる。
permission変更・satp切替・SFENCE.VMA・FENCE.Iの後に古いtranslationや命令を使わないことを試す。
Sv39対応時はPCだけでなく、予測器のtag/target、復帰先、例外情報、LSUの元VAも見直す。
物理アドレスは32 bitのままにできるが、上位PPNを無条件に切り捨てて別名にしてはならない。

### 4. 実際のOSで必要になった拡張

F/DはFP演算器だけでなくFS状態・context save/restore・ABIと一緒に導入する。
V、H、SMP、PLIC/AIA互換性、Linux/OpenSBI互換性は今の自作OS対応には含めない。
必要が具体化した時点で優先順位を更新する。外部RAMやUARTはISAとは別の実装工程として並行できる。

## 全段階で維持する検証

- 独立整数oracle、Veryl/Celoxとmapped回帰、命令単位とCPU統合の両方を確認する。
- ACT/Sailと実OS試験を追加する。既存236 ELF（従来234＋割り込み2本）の同一byte回帰を維持する。
- CPU 124.8 MHz／メモリ62.4 MHz、setup・hold各200 ps予約、JTCK照合を維持する。
- 機能モデル合格、配線後STA合格、checked bitgen、exact hash実機の順で認定する。
  新imageの既定化はその後。IPC×clockと同じ仕事の実行時間を記録する。

## 参照仕様

2026-09-10確認。仕様の許容範囲と上の実装方針は区別する。

- [Machine ISA 1.13](https://docs.riscv.org/reference/isa/v20260120/priv/machine.html)：
  misa WARL、割り込み、MRET、WFI、PMP。
- [Supervisor ISA 1.13](https://docs.riscv.org/reference/isa/v20260120/priv/supervisor.html)：
  S-mode CSR、satp、Sv39、SFENCE.VMA。
- [A 2.1](https://docs.riscv.org/reference/isa/v20260120/unpriv/a-st-ext.html)：
  Zaamo/Zalrsc、順序、reservationと進捗。
- [M 2.0](https://docs.riscv.org/reference/isa/v20260120/unpriv/m-st-ext.html)：
  DIV/REM、zero/overflow、word結果。
