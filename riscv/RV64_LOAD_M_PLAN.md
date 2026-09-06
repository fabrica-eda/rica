# RV64 load待ち短縮・タイミング余裕・M拡張

2026-09-07。基準commitは `97503e3`。採用構成は **`HardwareCompactM4KTestTop`**：
scalar RV64、compact 64-entry BTB／2-bit方向予測／8-entry RAS、4 KiB L1I、
`FAST_LSU=5`、`ENABLE_M=1`。CPU **124.8 MHz**／memory **62.4 MHz**を維持した。

通常STAは **nominal setup +206 ps／hold +290 ps**。追加setup uncertaintyを200 psとした
実際の合否判定でも **setup +6 ps**で通過する。JTCK 6 MHzの515 setup／515 holdを含み、
全体で15,171 setup／15,171 holdを検査した。既存の4境界例外は変更していない。
200 psは設計上の余裕目標であり、未測定のPVT全条件を保証する値ではない。

通常bitgenを通った同一SHAのbitstreamで **133種のELF、再測定込み165実行が実機合格**。
最終RTLから再合成して配線を再読込し、通常STA・JTCK照合・bitgenを繰り返して、
実機に書いたものと同じbitstreamを再生成した。モデルの合格だけで認定した結果ではない。
[コマンド、ソース・ELF・bitstreamのhash、ログと保存物](../toolchain/measurements/2026-09-06-rv64-load-margin-m/README.md)を記録した。

## 採用した変更

### load応答を1 memory edge短縮

CPUがACKまで保持するLSUアドレスをBRAM port Bへ1 edge早く提示する。
要求tokenとstoreのdata／maskは登録済みのものを使い、storeは1回だけ実行する。
loadデータは従来のmemory側出力レジスタを経由するので、BRAM出力の大きな選択回路を
そのままCPUまで延ばさず、応答を3 memory edgeから2 edgeへ短縮できる。
直前storeとの番地一致に限らず、通常のloadに効果がある。

候補比較として、直近SDの転送、出力レジスタを外す方式、CPU側で応答を再登録する方式も検査した。
出力レジスタを外した方式はnominal setup −1,983 psで未達。CPU側登録はstore ACKを
余分に遅らせない修正後もload性能が弱く、採用しなかった。比較用の`FAST_LSU=0..4`は
明示選択で残し、今回の既定実機imageは5を使う。

### RV64Mの13命令

`MUL/MULH/MULHSU/MULHU/MULW`、`DIV/DIVU/REM/REMU`、
`DIVW/DIVUW/REMW/REMUW`を実装した。ISA仕様は
[RISC-V M v2.0](https://docs.riscv.org/reference/isa/unpriv/m-st-ext.html)に従う。
除数0、最小負数÷−1、signed／unsignedの上位積、W結果の符号拡張を扱う。
予約済みword命令を拒否し、M無効時はすべてのM命令をillegal instructionにする。
`misa.M`は`ENABLE_M`に合わせた読み取り専用値とした。

乗算はradix-4で32反復、除算は復元除算で64反復（Wは32反復）。符号処理と結果登録には
別のcycleを使う。CMに未完了M命令がある間は若い命令を止め、完了前の値をbypassしない。
古い分岐・例外が優先され、破棄された演算はレジスタ、メモリ、CSRを更新しない。
M待ちは既存dependency counterへ含め、memory-wait counterとは区別する。

初期実装ではCMの比較から開始判定を経て128-bit入力レジスタへ至る制御経路が遅かった。
採用版ではidle中の入力payloadを毎cycle更新し、開始判定は演算の所有を示す状態だけに掛ける。
CMの結果payloadもM待ち中は更新し、完了まで消費させない。この変更で広い保持muxと
開始／cancel信号の負荷を減らした。古いCM redirectによる開始抑制は残した。
投機開始を許す別案もSTA +202 psまで達したが、実機確認した+206 psの方式を採用した。

### 既存の良い配置・配線を再利用

Texoへ`--resume-checkpoint`を追加し、良い物理実装から局所配置修正と配線修正を交互に行う。
cell名・BEL、driverの物理pin、PIP、全sink、必須clock経路を現在の設計と照合し、
古いtiming結果は使わずSTAを再計算する。clock、delay model、検査範囲を緩める変更はない。
Mなしの基準回路も、元の+6 psから**+209 ps**へ改善し、133実機実行に合格した。

修正は[Texo PR #67](https://github.com/fabrica-eda/texo/pull/67)へ提出した。
物理測定はStruo `eff404d`／Veryl 0.20.3に固定したTexo `eb72ee2`で実施し、
PRは最新mainのStruo `836236d`／Veryl 0.21へ載せ直してtest／Clippyを通した。
この依存差を含め、バイナリとソースのhashは証跡に保存した。

## 実機IPCと処理時間

各診断を3回測定した中央値。区間は実機で凍結した`mcycle`／`minstret`で、
reset trampoline・起動・結果検査を含む。同じELF hash、同じCPU／memory clockで比較した。
モデルのfirst-commit以降のexecution windowとは区間が異なり、数字を混ぜていない。
基準はMなしFAST_LSU=2を+209 psにしたimage。IPCは`minstret/mcycle`、
MIPSは`IPC × 124.8`である。

| 診断 | mcycle | minstret | IPC | IPC × clock（MIPS） | 基準比速度 |
| --- | ---: | ---: | ---: | ---: | ---: |
| alu_chain | 5,993 | 4,616 | 0.770232 | 96.1249 | 1.0000x |
| branch_pattern | 8,642 | 2,825 | 0.326892 | 40.7961 | 1.0001x |
| call_return | 4,947 | 1,546 | 0.312513 | 39.0016 | 1.0000x |
| load_store | 4,423 | 1,290 | 0.291657 | 36.3988 | 1.1158x |
| array_sum | 14,760 | 5,152 | 0.349051 | 43.5616 | 1.1387x |
| array_copy | 22,475 | 7,464 | 0.332102 | 41.4464 | 1.1138x |
| pointer_chase | 12,658 | 4,113 | 0.324933 | 40.5516 | 1.1618x |
| call_nested | 17,281 | 5,135 | 0.297147 | 37.0840 | 1.0592x |

従来の固定4診断の速度比幾何平均は **1.027792（+2.78%）**。
すべてのプログラムが速くなるとは主張しない。一部ACT4のcycleはcache／prefetchの位相が
変わることで増減した。表の既存ALU／分岐／call診断はほぼ同等、memory診断は改善した。

Mの評価は同一256組の入力を処理するソフトウェア実装との比較も行った。
ソフト乗算は0になるまでのshift/add、除算は除数を正規化してからshift/subtractする参照実装。
全compiler runtimeを代表する比較ではない。両版のload、call/return、checksum loopは共通である。

| 同じ仕事の実装 | mcycle | minstret | IPC | MIPS | 時間（µs） | M版の速度比 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ソフト乗算 | 116,590 | 43,915 | 0.376662 | 47.0074 | 934.215 | — |
| M乗算 | 16,294 | 2,580 | 0.158340 | 19.7609 | 130.561 | **7.1554x** |
| ソフト除算 | 221,465 | 92,760 | 0.418847 | 52.2721 | 1,774.559 | — |
| M除算 | 24,486 | 2,580 | 0.105366 | 13.1497 | 196.202 | **9.0446x** |

M版は1命令あたりのcycleが多くIPCは下がるが、必要な命令数が減り、処理時間は大幅に短い。
M導入の採否をMIPSだけで判断しない。

## 検証と再実行

- 最終ソースのRica test：**167 pass、既存ignore 1**。
- 演算器：独立した整数oracleの**3,237組**、cancel／結果保持。
- Core：連続演算、依存、store、古い分岐による破棄、例外、予測修復と演算所有、`misa.M`。
  TAGE、bimodal、static、compactで検査。
- 最終board-memory RTLモデル：**133 ELF × 両clock順 = 266 pass**。
  過去のFF更新順序不具合を露呈した連続debug writeもnative/WASMで回帰検査した。
- 実機：既存100、memory 3、独自M 13、ACT4 M 13、性能比較4の**133 ELF**。
  16診断をさらに2回測り、計**165 pass**。既定runnerへ切り替えた後の8 passを加えて**合計173実行**。
  MのACT4期待値はSailで生成した。
- 最終ソースのVeryl check、Rust fmt、変更したRust targetのClippyは合格。
  全target Clippyは変更していない3ファイルにある既存5警告で失敗し、全面合格とは記載しない。

```sh
cargo +1.98.0 test --offline --locked --release -p verify-riscv
cargo +1.98.0 run --offline --locked --release -p verify-riscv --bin rv64-riscv-tests -- \
  --m-tests --max-cycles 1000000
cargo +1.98.0 run --offline --locked --release -p verify-riscv --bin rv64-riscv-tests -- \
  --act4-suite M --max-cycles 1000000
cargo +1.98.0 run --offline --locked --release -p verify-riscv --bin verify-riscv -- \
  --hardware-memory-rtl --compact-predictor --icache-kib 4 --fast-lsu 5 \
  --m-extension --max-cycles 1000000 riscv/build/rv64-m/*.elf
cargo +1.98.0 run --offline --locked --release -p verify-riscv --bin hardware-riscv-tests -- \
  --xvc 127.0.0.1:3721
```

既定bitstream：`build/riscv-hardware-compact-m-load-4k-124p8-20260907.bit`。
SHA-256：`7aef47d3c5d8736b5730250053e313300aef1f18d44442157f66678ebf6f53f5`。
ビルド省略時は証跡の圧縮保存物からこの同一imageを復元できる。

## 次の改良計画

1. **M乗算の短縮を優先**。今回のMUL診断では9,219 cycleがdependencyイベントに計上された。
   演算器のstart／done／cancelの契約を維持し、ECP5 DSPを使う部分積の分割・登録を試作する。
   配線後の124.8 MHz、200 ps追加余裕、同じ仕事の時間と面積で今のradix-4版と比較する。
   DSP mappingが使えない場合は反復幅拡大やMULW短縮を独立に比較する。
2. **除算の早期完了**。0除算、絶対値の大小、上位0などを使う短縮を検討する。
   現行の64／32反復を基準として全corner caseとデータ分布別の時間を比較する。
   MULH＋MUL、DIV＋REMの結果再利用はその後に扱う。
3. **命令拡張はZba→Zbb/Zbsを候補**（[公式bit操作仕様](https://docs.riscv.org/reference/isa/unpriv/b-st-ext.html)）。配列アドレス計算、bit操作の命令数削減を確認し、
   長い64-bit muxやshift経路を増やすなら別stageへ分ける。個々の拡張の全命令が揃ってから
   その拡張対応を宣言し、ACT4の独立catalogを追加する。
4. **A・特権・浮動小数点は用途に合わせて順番を決める**。Aはreservation、例外、外部書込みと
   メモリモデルを設計する必要がある。OS用途なら特権／interrupt／MMUも含む別計画にする。
   現在の圧縮実装は整数Zcaであり、F/D込みのC全体を実装したという表記にはしない。

各段階で同一ELFのIPC×clockと、命令数が変わる場合の同じ仕事の処理時間を両方記録する。
今回の+206 ps合格imageを基準にし、未達候補からbitstreamを生成して採用する運用は行わない。
