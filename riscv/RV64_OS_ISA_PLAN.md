# 自作OSを動かすためのRV64 ISA・プラットフォーム計画

2026-09-10。自作OSを前提に、整数命令の追加より特権・割り込み・メモリ保護を優先する。
ユーザーの選択により **M/S/U-mode＋Sv39による保護・仮想記憶を持つOS** を目標として確定した。
最初はM-mode基盤を完成させ、段階的にS-mode kernelとU-mode processへ進む。
現時点でS-modeやMMUが既に使えるとは扱わない。

## 現在地

認定済み基準を **`HardwareCompactAccessFaultTestTop`** に更新した。
CPU **124.8 MHz**、メモリ62.4 MHz、JTCK 6 MHz。
setup・hold各200 ps予約後で **setup +2 ps／hold +90 ps**、各19,032検査に合格した。
通常Texoが試作ツールの配置・配線・STAを含むチェックポイントをバイト単位で再現し、
JTCKも別構築グラフの各515検査と一致した。STAモデル・例外・予約値は変更していない。

同じbitstreamで **238 ELF＋反復78回＝316実機実行が合格**した。
標準起動設定に切り替えた後の16実行も合格し、合計332実行を確認した。
通常診断35本は従来の認定済み実機と同一ELF・同一命令数で、中央値の差は最大±1 cycle。
64-bitアドレス検査を追加しても通常処理の性能を維持している。
Rust kernelも8種類のfaultから復帰して2タスクを完了し、両variantが各3回合格した。
[認定記録](../toolchain/measurements/2026-09-10-rv64-access-faults/qualification.json)・
[実機IPCとクロック](../toolchain/measurements/2026-09-10-rv64-access-faults/PERFORMANCE.md)。

| 分類 | 認定済み基準 | OS向けに必要な差分 |
| --- | --- | --- |
| 整数ISA | RV64 I/M/B、整数圧縮Zca、Zicsr、Zifencei | A（Zaamo＋Zalrsc）。F/D/Vは後工程 |
| 特権 | M-mode、ECALL/EBREAK/不正命令、MRET | M/S/Uの実行権限、CSRアクセス権、例外委譲 |
| CSR | mie/mip、64-bit mepc/mtvalと割り込み原因 | S-mode CSR、権限と委譲、time alias |
| 割り込み | MSI/MTI/MEI受付、MMIO timer/MSIP。MSI/MTIとプリエンプションは実機認定、MEIはCPU試験 | 外部デバイス/controllerの接続 |
| アドレス | PC・分岐先・EPC・mtvalは64 bit。物理要求は範囲検査後の32 bit | 仮想アドレス変換と変換後の物理幅検査 |
| メモリ | 252 KiB RAM＋4 KiB reset領域、厳密なRAM/MMIO decodeとaccess fault | PMP、ページfault |
| 保護・仮想記憶 | PMP/MMU/TLBなし | U-mode隔離、S-mode、Sv39、SFENCE.VMA |
| I/O | 停止中のJTAG loader、試験結果観測、MMIO timer | UART、IRQ制御、必要に応じ外部RAM |

`STRICT_MEMORY=1` は上位アドレスの別名、範囲を跨ぐ部分store、誤予測fetchによる
誤ったtrapを防ぐ。通常load/storeの待ち段は増やしていない。
今回採用した `carry-match` は、分岐先加算のcarry両候補を先に比較し、最後に1 bitの
一致結果を選ぶ。既存のEX/CM境界を保ち、追加サイクルなし、比較変更による増加は10セル。
最終構成は33,474セル、8,991 FF、16 DSP、150 EBRである。

このRTLでCPU 61件、Veryl check、厳密構成476実行・無効構成468実行が合格した。
同時edgeモデルの通常35本は基準モデルと命令数・サイクル数が一致する。
実機のkernel中央値はtasksが86,653 cycles／30,725 instructions、
access-faultsが92,235 cycles／31,949 instructionsで、現在の同時edgeモデルとも一致した。
タイマーで処理量が変わるkernelは固定処理のIPC比較から分けて記録する。
[機能検証の対応](../toolchain/measurements/2026-09-10-rv64-access-faults/carry-match-functional-validation.json)。

### 配置配線の改善と次の整理

部分配置で未指定セルを最初の空きBELへ置く処理を、接続先を考慮した配置補完に分けた。
新しいLUTが遠くへ配置される問題を解消し、同じRTLの初期setup不足を12,205 psから120 psへ縮めた。
詳細配置では同じタイルの候補だけで探索枠を使い切らず、実配線遅延を基準に移動を評価する。
配線では1 ps単位のコストを使い、共有経路の所有者を最大32本まで合法に引き直す。
同じ優先度なら改善対象を先に配線し、元の所有者が直ちに経路を取り返す問題も修正した。
最終的にtrial 98がsetup +2 psへ到達し、通常Texoのtrial 99が全体を再現した。

次は試作ツールの配置補完・詳細配置・配線修正を、回帰試験とともに通常フローへ整理する。
今回の良好な配置・配線を次のRTL変更の初期候補として使い、変更後の全資源とSTAを検査し直す。
大域配置の目的関数の改良は複数設計で比較する。このCPUでの成立を汎用的な性能保証とは扱わない。
局所配置のタイル分散をさらに広げる `local-diversity` は224＋75件の試験に合格したが、
今回の認定bitstreamには使っていない。試作の不採用理由・再現手順・ソースは
[配置配線の全記録](../toolchain/measurements/2026-09-10-rv64-access-faults/README.md)に残した。

## 実装順と合格条件

### CPUとOSを同じ開発ループに置く

最小kernelは **Rust `no_std`＋起動・trap入口のアセンブリ** で実装する。
CPU機能の追加と、それを使うkernel経路・自己検査ELFを一組で進める。
M-mode固有のCSR・timer操作はアーキテクチャ層へ置き、後のS-mode移行に備える。
初版は固定メモリと少数タスクを用い、起動・例外記録・yield・タイマー切り替えを持つ。
Rustの`core`もAを含まないターゲットでビルドし、生成ELFを命令単位で検査する。

CPU側のアドレス検査とaccess fault、kernel側の元PC・原因・アドレスの記録、
意図的なfaultからの復帰、タスク継続まで実機認定した。以降はAとatomic操作、U/PMPとsyscall・隔離、
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
厳密なメモリdecode/access faultとRust kernelの例外復帰も実機認定した。
この工程の残りは外部I/O。S/U/PMP/Sv39は後工程のまま。

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

次は外部I/Oを小さな単位で追加し、Rust kernelから起動・例外情報を観測できるようにする。
その後A、下位特権・PMPへ進む。64-bit上位アドレス、境界を跨ぐaccess、
誤予測fetch、割り込みとfaultの競合は既存の回帰試験として維持する。

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
