# 自作OSを動かすためのRV64 ISA・プラットフォーム計画

2026-09-11。自作OSを前提に、整数命令の追加より特権・割り込み・メモリ保護を優先する。
ユーザーの選択により **M/S/U-mode＋Sv39による保護・仮想記憶を持つOS** を目標として確定した。
最初はM-mode基盤を完成させ、段階的にS-mode kernelとU-mode processへ進む。
現時点でS-modeやMMUが既に使えるとは扱わない。

## 現在地

2026-09-23: [STA表全体の実測モデルへの置換](characterization/STA_REPLACEMENT.md)を完了。
全1,552項目を474経路（学習261・検証213）で検証し、旧数値表への補完は使わない。
共通マージンを201 psから194 psへ減らし、温度余裕20%は維持した。
CPU 124.8 MHz／メモリ62.4 MHzでsetup +2 ps／hold +1 ps、JTCK各515件一致、
267実機試験、266件の命令数一致・サイクル差±1以内、コンソール686 byte一致を確認。
[採用プロファイル](qualified-measured-sta.json)を既定入口へ反映した。
以下は比較元となる段階別の記録。

2026-09-22（JST）: `HardwareCompactAtomicTestTop` のA拡張とRust atomic OSを
**CPU 124.8 MHz／メモリ62.4 MHz／JTCK 6 MHz** で実機認定した。
setup/hold各200 ps予約後は **0/+90 ps**、JTCK各515検査も一致。
A診断24件＋既存238件＋OS 5種類の **267実機実行が合格**した。
同一ELFの既存238件は旧実機と命令数が一致し、サイクル差は全件±1以内。
コンソール686 byteもモデルと一致した。AMO/LR/SCとそれを使うOSの段階を完了し、
次はU/PMPによる隔離、続いてS/Sv39へ進む。
[認定済みAイメージと実行手順](RV64_ATOMIC_PLATFORM.md#hardware-qualification-2026-09-22-jst)。
実行時はA対応bitstreamを明示する。runnerの既定値は旧consoleイメージのまま。

2026-09-22: [実測モデルをTexo本体の配置改善へ組み込んだ](characterization/REPLACEMENT.md)。
Rica用の入口 `riscv/tools/texo-measured` で旧配置予測式を無効にし、実測入力のハッシュ検査、
合法な配置候補の生成、再配線とSTA、悪化時の取消しを行う。
CPUでLUT移動を採用し、124.8 MHz、setup/hold各200 ps予約、残余0/+90 psを維持した。
新しいbitstreamで267試験に合格し、[採用プロファイル](qualified-measured-placement.json)に記録した。
基板にはこのCPUを残し、最後のatomic OSはmcycle=127145 / minstret=41092で合格した。
この段階ではSTA表の全面置換は未完了で、不足項目を `coverage-report.json` に明示した。
現在は上記の追加測定・全表置換・実機認定によって置換済み。

2026-09-22: [FPGA実測の配置コストモデル](characterization/README.md)を追加した。
96経路・408測定から学習し、別の24経路で最大誤差4.25%。実測モデルが選んだ
LUT移動を、旧配置予測を無効にしたPnRへ渡し、同じクロックと200 ps予約で
setup 0 ps／hold +90 ps、JTCK各515検査の一致を確認した。
これは配置候補の評価モデルであり、STA用の全タイミング表を置換したものではない。
FPGAは従来の認定済みAイメージへ戻し、atomic OSを再確認済み。

同日、[FF・BRAM・DSP・負荷配線の実測](characterization/PHASE_APERTURE.md)を追加した。
PLL位相を約200 ps刻みで変え、新値・旧値・混合値を極性別に測定。
114イメージから31経路の捕捉境界を採用し、温度分20%＋200 psの仮定マージンを
加えた11種の経路予算は、学習に使わない16経路の検証にも合格した。
モデルが選んだ早い側・遅い側の位相を各3回の再書込みで確認した。
これはclock-to-Q・配線・setup/hold・クロックスキューを含む経路全体のモデルで、
独立したSTAアークや電圧依存性の測定値へは分解しない。
CPUの周波数・RTL・STAは変更せず、計測後のatomic OSも合格した。

### 前工程のconsole認定（2026-09-11）

認定済み基準を **`HardwareCompactConsoleTestTop`** に更新した。
CPU **124.8 MHz**、メモリ62.4 MHz、JTCK 6 MHz。
setup・hold各200 ps予約後で **setup +2 ps／hold +90 ps**、各19,690検査に合格した。
探索変更を含まない配置配線・STAがチェックポイントをバイト単位で再現し、
JTCKも別構築グラフの各515検査と一致した。STAモデル・例外・予約値は変更していない。

同じbitstreamで **238 ELF＋反復78回＋console kernel 3回＝319実機実行が合格**した。
標準起動設定に切り替えた後の8実行も合格し、合計327実行を確認した。
通常診断35本は従来の認定済み実機と同一ELF・同一命令数で、中央値の差は最大±1 cycle。
実行中のUSB/JTAGコンソールとMEI受信を追加しても、通常処理の性能を維持している。
Rust kernelは起動・8種類のfault・受信応答を出力し、2タスクを完了した。
consoleの3実行はモデルと同じ686 byteのログ、既存の両variantも各3回合格した。
[認定記録](../toolchain/measurements/2026-09-11-rv64-console/qualification.json)・
[実機IPCと設定クロック](../toolchain/measurements/2026-09-11-rv64-console/PERFORMANCE.md)。

| 分類 | 認定済み基準 | OS向けに必要な差分 |
| --- | --- | --- |
| 整数ISA | RV64 I/M/B、整数圧縮Zca、Zicsr、Zifencei | A（Zaamo＋Zalrsc）。F/D/Vは後工程 |
| 特権 | M-mode、ECALL/EBREAK/不正命令、MRET | M/S/Uの実行権限、CSRアクセス権、例外委譲 |
| CSR | mie/mip、64-bit mepc/mtvalと割り込み原因 | S-mode CSR、権限と委譲、time alias |
| 割り込み | MSI/MTI/MEI受付、MMIO timer/MSIP、console受信MEIとプリエンプションを実機認定 | 複数デバイスのIRQ制御 |
| アドレス | PC・分岐先・EPC・mtvalは64 bit。物理要求は範囲検査後の32 bit | 仮想アドレス変換と変換後の物理幅検査 |
| メモリ | 252 KiB RAM＋4 KiB reset領域、厳密なRAM/MMIO decodeとaccess fault | PMP、ページfault |
| 保護・仮想記憶 | PMP/MMU/TLBなし | U-mode隔離、S-mode、Sv39、SFENCE.VMA |
| I/O | JTAG loader、MMIO timer、実行中のUSB/JTAGコンソール（TX/RX各16 byte） | UART、必要に応じ外部RAM |

`STRICT_MEMORY=1` は上位アドレスの別名、範囲を跨ぐ部分store、誤予測fetchによる
誤ったtrapを防ぐ。通常load/storeの待ち段は増やしていない。
前工程で採用した `carry-match` は、分岐先加算のcarry両候補を先に比較し、最後に1 bitの
一致結果を選ぶ。既存のEX/CM境界を保ち、追加サイクルなし、比較変更による増加は10セル。
コンソール追加後の最終構成は34,829セル、9,345 FF、16 DSP、150 EBRである。

console有効/無効の238 ELF×2 clock順序、計952実行とVeryl checkが合格した。
同一37 ELFの同時edgeモデルは追加前と命令数・サイクル数が一致する。
実機のkernel中央値はtasksが86,653 cycles／30,725 instructions、
access-faultsが92,235 cycles／31,949 instructionsで、現在の同時edgeモデルとも一致した。
タイマーやhost応答で処理量が変わるkernelは固定処理のIPC比較から分けて記録する。
[機能検証と実機の対応](../toolchain/measurements/2026-09-11-rv64-console/README.md)。

### 前工程の配置配線改善と通常フローへの統合

部分配置で未指定セルを最初の空きBELへ置く処理を、接続先を考慮した配置補完に分けた。
新しいLUTが遠くへ配置される問題を解消し、同じRTLの初期setup不足を12,205 psから120 psへ縮めた。
詳細配置では同じタイルの候補だけで探索枠を使い切らず、実配線遅延を基準に移動を評価する。
配線では1 ps単位のコストを使い、共有経路の所有者を最大32本まで合法に引き直す。
同じ優先度なら改善対象を先に配線し、元の所有者が直ちに経路を取り返す問題も修正した。
最終的にtrial 98がsetup +2 psへ到達し、通常Texoのtrial 99が全体を再現した。

試作ツールの配置補完・詳細配置・配線修正を、通常の `texo pnr` とflow APIへ統合した。
Texo `2639484` にコミットし、459件の回帰、Clippy、整形に合格した。
部分配線は `--initial-placement`＋`--initial-routes`、setup探索時間は
`--setup-optimization-budget-seconds` で指定できる。時間制限は呼び出しごとに独立し、
打切り時にも実際のslackを報告する。0秒の未達対照ではbitgenを拒否した。
通常CLIの再現checkpoint・bitstreamは前回の認定済みSHAと一致し、追加の実機8実行も合格した。
setup −27 psのcheckpointからも通常CLIで+2 psへ収束し、同じ認定済みcheckpointを得た。
[統合の検証記録・再現手順・パッチ](../toolchain/measurements/2026-09-10-texo-closure-integration/README.md)。

コンソール追加では、初期配線を移動可能な候補として利用し、変更で遅くなる周辺経路も
まとめて修復した。全体のSTAで改善を確認してから採用し、最後の局所配置6件・配線3件で閉じた。
合格結果は元の配置配線・STAで完全に再現した。
[今回の探索・不採用候補とパッチ](../toolchain/measurements/2026-09-11-rv64-console/ROUTING.md)。

次はAとatomic操作、U/PMPとsyscall・隔離、S/Sv39とページ表・プロセス切り替えへ進む。
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

### 1. 完了：M-modeでプリエンプションできる最小基盤

`ENABLE_INTERRUPTS=1` と `HardwareCompactInterruptTestTop` に割り込み・タイマー基盤を実装。
[CSR、受付境界、MMIO、resetの仕様](RV64_MACHINE_PLATFORM.md)。
2タスクの全整数contextを16回切り替える試験は、両方のシミュレーションclock順序、
同時edgeモデルと実機3回で合格した。124.8 MHz、setup・hold各200 ps予約を維持した。
[STA・実機の認定記録](../toolchain/measurements/2026-09-10-rv64-machine-interrupts/README.md)。
厳密なメモリdecode/access faultとRust kernelの例外復帰も実機認定した。
実行中のUSB/JTAGコンソールとMEI受信も実機認定した。S/U/PMP/Sv39は後工程のまま。

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

Rust kernelから起動・例外情報を観測できるようになったため、次はA、下位特権・PMPへ進む。
64-bit上位アドレス、境界を跨ぐaccess、
誤予測fetch、割り込みとfaultの競合は既存の回帰試験として維持する。

2026-09-11: [USB/JTAGコンソール](RV64_CONSOLE_PLAN.md)のRTL、Rust driverとMEI受信処理を実装。
起動・8件の例外ログと2タスク完走は3種類のclock評価と実機3回で合格した。
同じ124.8/62.4 MHz、setup/hold各200 ps予約で配置配線・実機認定を完了した。

### 2. A拡張：OSのlockとコンパイラのatomic

2026-09-21: AMO演算部、decode/LSUへの接続とLR/SCを実装し、opt-in構成で
`misa.A` を公開する。Rust atomicを使うOS版とnative/ECP5 mapped試験も追加済み。
2026-09-22（JST）に新しい構成も同じ124.8 MHzで物理・実機認定を完了した。
[認定済みA構成の仕様・検証範囲](RV64_ATOMIC_PLATFORM.md)。
OS側は既存の非ネストtrapを使うfetch-add syscallを追加し、認定済み124.8 MHz
imageで動作を確認済み。[今回の検証と残作業](RV64_OS_ISA_PROGRESS_20260921.md)。

`LR.W/D`、`SC.W/D` と全AMO.W/Dを実装し、Zaamo＋Zalrscが揃ってから `misa.A` を立てる。
単一hartの初版はLSUで直列化し、`aq/rl` とFENCEの順序を維持する。
reservationは物理アドレスで追跡し、SCの成功/失敗、word結果の符号拡張、alignment fault、
途中trapとcontext switch、外部書込みによる無効化、constrained LR/SC loopの進捗を検証する。
MMIOのatomic可否はメモリ属性で判定する。

単一hartの最初のM-mode OSは割り込み禁止区間でも実装できるので、Aの完成をタイマ起動の前提にしない。
A対応後のソフトウェアターゲットは `rv64imac_zicsr_zifencei_zba_zbb_zbs`、ABIはまず `lp64`。
現行image用ビルドにA命令を混ぜない。

### 3. U-modeと保護、その後S-mode＋Sv39

2026-09-24: opt-in U/PMPと、隔離された2つのUタスクを実装中。
CSR/PMPのnative・mapped検証とOSの機能モデル検証は合格。
124.8 MHzでの配置配線・実機認定は継続中で、既定の認定imageは未変更。
[U/PMPの仕様・検証範囲](RV64_USER_PLATFORM.md)。

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
- ACT/Sailと実OS試験を追加する。既存238 ELFとconsole kernelの同一byte回帰を維持する。
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
