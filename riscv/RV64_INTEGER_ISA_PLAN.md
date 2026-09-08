# RV64整数ISAの拡充と検証計画

2026-09-08着手。2026-09-09にZba・Zbb・Zbsを実機認定し、B拡張一式の対応を完了した。
比較基準は `9526c1a` のDSP版、CPU 124.8 MHz／メモリ62.4 MHz、
setup・hold各200 ps予約、133種類のELF／181実機実行。

## 方針と現在地

整数ISAを **I/M/Zca → Zba → Zbb → Zbs** の順に拡充する。
拡張ごとに機能・タイミング・実機を確定し、コンパイラが生成する命令列で比較する。
**Zba／Zbb／Zbsは完了。RV64 B 1.0一式を124.8 MHzで実機認定した。**
現行構成・性能・次の計画は本書の「第3段階: Zbs 実機認定」にまとめた。
仕様は [RISC-V B v1.0](https://docs.riscv.org/reference/isa/unpriv/b-st-ext.html)。

現在の圧縮命令は整数Zca。M-modeの限定したCSR／同期例外も実装している。
A、特権／割り込み、F/D/Vは動かすソフトに合わせて別工程とする。
LSU・MDUの追加高速化は、整数拡張後のコンパイラ生成コードで効果を判断する。

## 第1段階: Zbaの構成

`HardwareCompactZbaDspM4KTestTop`：`ENABLE_ZBA=1`、`ENABLE_M=1`、
`M_USE_DSP=1`、`FAST_LSU=5`、64-entry BTB／2-bit方向予測／8-entry RAS／4 KiB L1I。
Zba無効時は該当命令を不正命令として扱う。

- `add.uw`、`sh1add/sh2add/sh3add`、`sh1add.uw/sh2add.uw/sh3add.uw`、`slli.uw`の8命令。
- `.uw`はrs1の下位32 bitを演算前にゼロ拡張し、結果は64 bit。`slli.uw`のshiftは0〜63。
- 加算は既存の32-bit × 2 carry-select加算器を共用する。0〜3 bitの入力シフトを
  2段の二択muxで作る。専用加算器も比較し、物理条件を通した共用版を採用した。
- `slli.uw`は既存の二段シフタを使い、未完成の結果を転送しないよう依存判定を更新した。
- CM分岐比較からMDU開始への長い経路を除いた。誤予測先で開始したM命令は、
  登録済みredirectでPREPARE中にcancelする。実際の開始とcancelを通る回帰試験を追加した。
  system例外はEXで後続をflushするため、後続M命令が開始しないことも確認した。
- RASの定数slot計算をgenerate内の定数にし、不要なarithmetic cellsを16個除いた。
  [小さな再現](../toolchain/mre/struo-constant-loop/README.md)ではmapped cellsが144→8になり、
  全入力で値が一致する。依存ライブラリは変更していない。

## Zbaの物理条件と検証結果

| 項目 | 認定結果 |
| --- | --- |
| FPGA | LFE5UM5G-85F / CABGA381 / speed 8_5G |
| CPU / memory / JTCK | **124.8 / 62.4 / 6 MHz** |
| setup：nominal / 200 ps予約後 | **+204 / +4 ps** |
| hold：nominal / 200 ps予約後 | **+290 / +90 ps** |
| setup / hold検査数 | 各16,964。うちJTCK各515。既存の明示的例外4件 |
| セル数 / DSP / PIPs | 26,024 / 16 / 319,919（基準からセル数349減） |
| workspace回帰 | **178 PASS / 1 ignored**。Clippy、Veryl checkも合格 |
| Zba整数oracle | RTLとmappedモデルでそれぞれ**15,695組**合格 |
| CPU＋メモリ機能モデル | **153 ELF × 2 clock順 = 306 PASS** |
| 認定bitstreamの実機 | **153 ELF + 診断40反復 + 既定runner 16実行 = 209 PASS** |

153 ELFは基準133本を同一SHAで維持し、自作Zba 8本、公式ACT4/SailのZba 8本、
C診断4本を追加したもの。Windows D2XX/XVCで書き込み・実行した。
ACT4は `f7ba06c3d1a01428b172fe851ab56931a0624fee`、専用の
`act4/test_config_zba.yaml` とZba有効Sail設定を使う。

JTCKは別構築のグラフでnominal slackとedgeを照合した。同じSTAエンジンとdelay dataを
使う検査であり、独立した遅延characterizationではない。JTAGG外部境界／reset／CDCの
扱いと4件の例外は従来どおりで、検査対象を減らしてタイミングを通してはいない。

採用候補は `zba-15`。現行33 RTLファイルから合成し直した `zba-18-final` は、
通常の配置・配線検査とSTA、checked bitgenを経てcheckpointとbitstreamがbyte単位で一致した。

- 既定image：`build/riscv-hardware-compact-zba-dsp-m-4k-124p8-20260909.bit`
- bit SHA-256：`b761695a1af8496c2ea042d1dcd909941a70f5b13e98652baa04e5e82c9dd1d2`
- checkpoint SHA-256：`7cbeb0a40b4b71c1a58c492c565fb795577a3449af71026141137fbd4cc97f70`

## Zbaの実機性能

同じCソース・入力・検算をClang 20.1.2、`-O2 -mabi=lp64 -mcmodel=medany`でコンパイルし、
`-march=rv64imc_zicsr_zifencei`にZbaを加えた場合と比較した。
Zbaなしに新命令が混入しないこと、Zbaありで`sh1add`／`sh2add`／`sh3add.uw`が
生成されることをdisassemblyで確認した。測定は同じ認定image上の3回中央値。
起動・検算を含む、実機のfreeze済みCSR `mcycle`／`minstret`を使う。

| 同じ仕事 | cycles：Zbaなし → あり | 実行時間：なし → あり | 高速化 | 時間短縮 |
| --- | ---: | ---: | ---: | ---: |
| Cのアドレス計算 | 18,999 → 14,903 | 152.236 → 119.415 µs | **1.27484倍** | **21.559%** |
| Cの配列gather | 18,991 → 16,942 | 152.171 → 135.753 µs | **1.12094倍** | **10.789%** |

| 診断／ISA | retired | IPC | IPC × 124.8 MHz（MIPS） |
| --- | ---: | ---: | ---: |
| addresses / base | 10,296 | 0.541923 | 67.632023 |
| addresses / Zba | 7,224 | 0.484735 | 60.494880 |
| gather / base | 7,222 | 0.380285 | 47.459618 |
| gather / Zba | 6,198 | 0.365836 | 45.656381 |

命令数が変わるため、IPCやMIPSの増減だけでは同じ仕事の速さを比較できない。
今回の2診断では命令数と実行時間がともに減った。今後も仕事当たりの実行時間を主指標とする。
基準imageとの同一ELFの16診断は、全て中央値の差が±1 cycle以内。
乗算診断は10,146 cycles、2,580 retired、IPC 0.254287／31.735068 MIPS。
固定4診断の速度比幾何平均は0.999949で、起動位相差の範囲だった。

## 同時clockモデルの修正

Celox `7ff0dd2` の `Simulation::schedule` は、同時刻のFF domainを全評価→全更新で扱う。
Ricaが単一eventの手動`tick`を連続して呼んでいたことが、旧モデルと実機の大きな差の原因だった。
依存を変更せず、`--hardware-memory-rtl --timed-clocks`を追加した。
通常の高速な機能回帰はnative imageを再利用し、両clock順を検査する。
時刻付き診断は指定したELFごとにコンパイルする、明示的な性能診断として扱う。

| 診断 | 同時edgeモデル mcycle | 実機 mcycle | モデル／実機 minstret |
| --- | ---: | ---: | ---: |
| m_multiply（基準imageとの照合） | 10,147 | 10,147 | 2,580 / 2,580 |
| alu_chain（基準） | 5,993 | 5,992 | 4,616 / 4,616 |
| array_sum（基準） | 14,761 | 14,760 | 5,152 / 5,152 |
| addresses-base（基準） | 18,999 | 18,999 | 10,296 / 10,296 |
| gather-base（基準） | 18,991 | 18,991 | 7,222 / 7,222 |
| addresses-zba（認定image） | 14,903 | 14,903 | 7,224 / 7,224 |
| gather-zba（認定image） | 16,943 | 16,942 | 6,198 / 6,198 |
| m_multiply（認定image） | 10,147 | 10,146 | 2,580 / 2,580 |

乗算のメモリ待ちは1,793 cyclesで実機と一致した。旧モデルの1,285 cyclesとの差508 cyclesが
解消し、上記の照合は0〜1 cycle差になった。外部commitポートは終端storeのWBを待つ前に
観測を止めるためminstretより1件少ない。比較には同じCM retire境界のCSRを使う。
これは観測条件での整合であり、全clock位相や外部CDCの物理保証を追加するものではない。


## 第2段階: Zbb 実機認定（2026-09-09）

RV64 Zbb 1.0 全24命令を `ENABLE_ZBB` で実装し、実機認定した。
現行構成は `HardwareCompactZbbDspM4KTestTop`。ZbaとDSP Mを有効にし、
64-entry BTB／2-bit方向予測／8-entry RAS、4 KiB L1I、FAST_LSU=5を維持する。
`ENABLE_ZBB=0` では該当命令をprecise illegal-instruction trapとして扱う。

EXで論理・拡張・バイト操作、1/2/4 bit回転、byte単位のカウントを処理し、
CMで8/16/32 bit回転、カウント木、分割比較によるmin/maxを完了する。
結果は既存のWBレジスタへ格納する。依存命令はWBから受け取り、
直後と間に1命令ある依存の停止を明示した。独立したZbb命令は連続して発行できる。

CMからの共通転送を使う案はsetup −925 ps、Boolean式でカウントを小さくする案は
−724 psで停止した。どちらも200 ps予約後の値であり、採用していない。
採用したWB転送版は通常の配置修正で収束した。依存ライブラリのバージョンは変更せず、
初期checkpointを保存する診断用の8行も保存後に戻した。STAの条件を緩めてはいない。

| 項目 | 認定結果 |
| --- | --- |
| CPU / memory / JTCK | **124.8 / 62.4 / 6 MHz** |
| setup: nominal / 200 ps予約後 | **+200 / 0 ps** |
| hold: nominal / 200 ps予約後 | **+290 / +90 ps** |
| setup / hold検査数 | 各17,232。JTCK各515、既存の明示的例外4件 |
| セル / DSP / EBR / PIPs | 28,620 / 16 / 150 / 356,858。Zba版から2,596セル増 |
| workspace | **186 PASS / 1 ignored**。Veryl、Rust fmt、全targetのClippyも合格 |
| Zbb独立整数oracle | RTL／ECP5 mappedで各 **300,984組**合格 |
| CPU＋メモリモデル | **207 ELF × 2 clock順 = 414 PASS** |
| 認定bitの実機 | **207種類 + 診断52反復 + 既定起動16実行 = 275 PASS** |

207 ELFは基準153本を同一SHAで維持し、自作Zbb24本、公式ACT4/Sail Zbb24本、C診断6本を追加したもの。
Coreの試験には依存距離0〜3、両オペランド、若い命令の上書き、メモリ待ち、store、
誤予測先の破棄、M乗除算・CSRとの連続実行、無効設定と予約命令の例外を含む。
初回のMINU/MAX取り違えは公式試験が検出した。修正に加え、oracleのencodingも
外部LLVMアセンブラと照合するようにした。不合格試作からIPCを算出していない。

JTCKは別構築グラフとnominal slack／edgeを照合した。同じSTAエンジンとdelay dataを使うため、
独立した遅延characterizationではない。既存のJTAGG外部境界、reset／CDC、4件の例外は維持する。
workspaceの既存除外1件は、mapped board resetのNonScalarControlによるWRITE64試験である。
最終bitはWindows D2XX/XVCで書き込み、すべての実機試験を通した。

採用 `zbb-09` と、現行34 RTLファイルから合成し直した `zbb-12-final` は、
通常の物理検査・STA・JTCK照合・checked bitgenを通り、checkpointとbitがbyte単位で一致した。
モデル、STA、実機のRTL／ELF／bit hashも照合した。

- 既定image: `build/riscv-hardware-compact-zbb-dsp-m-4k-124p8-20260909.bit`
- bit SHA-256: `e0c122ae33f2b9565bbc933742c25fa7006117fc69a160e362db174c19ca1ea7`
- checkpoint SHA-256: `9a401e5ffc693413cf0029a881a11b97f44c21842f59397e71c8c27172e436cc`

### 同じC処理の実機時間

Clang 20、`-O2 -mabi=lp64 -mcmodel=medany`で、同じ入力・検算を持つCソースを
`rv64imc_zicsr_zifencei_zba` とZbb追加版へコンパイルした。
以下は同じ認定bit上の3回中央値。起動と検算を含むfreeze済みCSRを使う。
Zbbなしへの命令混入がなく、Zbbありで目的の命令が生成されることをdisassemblyで確認した。

| 同じ仕事 | cycles: base → Zbb | 時間: base → Zbb | 高速化 | 時間短縮 |
| --- | ---: | ---: | ---: | ---: |
| カウント | 187,276 → 27,211 | 1500.609 → 218.037 µs | **6.882364倍** | **85.470%** |
| 回転 | 25,166 → 19,007 | 201.651 → 152.300 µs | **1.324039倍** | **24.473%** |
| バイト処理 | 65,047 → 25,157 | 521.210 → 201.579 µs | **2.585642倍** | **61.325%** |

| 診断 / ISA | retired | IPC | IPC × 124.8 MHz（MIPS） |
| --- | ---: | ---: | ---: |
| counts / base | 96,370 | 0.514588 | 64.220594 |
| counts / Zbb | 18,487 | 0.679394 | 84.788416 |
| rotate / base | 15,429 | 0.613089 | 76.513518 |
| rotate / Zbb | 9,285 | 0.488504 | 60.965329 |
| bytes / base | 41,582 | 0.639261 | 79.779753 |
| bytes / Zbb | 15,416 | 0.612792 | 76.476400 |

回転・バイト処理はIPC／MIPSが下がっても、命令数の削減で仕事当たりの時間が短くなった。
基準Zba bitとの同一ELFの20診断は、全て中央値の差が±1 cycle以内。
乗算は10,147 cycles、2,580 retired、IPC 0.254262／31.731940 MIPS。
固定4診断の速度比幾何平均は0.999980で、起動位相差の範囲だった。
基準C3本も既存Zba bitで各3回検証し、新bitのbase版との差が±1 cycle以内である。

同時edgeモデルと実機のZbb診断は、カウント27,211、回転19,007、バイト25,157 cyclesで一致した。
乗算も10,147 cycles、memory wait 1,793 cyclesで一致した。これは観測条件での整合であり、
未測定の全clock位相や外部CDCを保証するものではない。

### 次の工程

1. **Zbs**: 単一bit操作と即値版を追加し、同じoracle／ACT4／依存・例外検証と物理ゲートを適用する。
   Zba/Zbb/Zbsがすべて認定された段階でB対応を宣言する。
2. **依存待ち・LSU・MDU**: ISA拡充後のCコードで再測定する。今回の回転診断はZbb結果の依存待ちが残る。
   単純なZbb演算の早い転送、load依存待ち、除算待ちを、124.8 MHzと200 ps予約を維持して比較する。
3. **ツール側**: Struoの小幅演算のマッピング、Texoの配置・配線の再利用と局所探索の改善余地を残す。
   回路を小さくしただけでは今回のSTAは改善しなかったため、実配線と同じ仕事の時間で採否を決める。

全26診断のIPC／clock、コマンド、失敗試作、再現artifactと照合結果は
[Zbb測定記録](../toolchain/measurements/2026-09-09-rv64-zbb/README.md)に保存した。

## 第3段階: Zbs 実機認定（2026-09-09）

`bclr/bclri`、`bext/bexti`、`binv/binvi`、`bset/bseti`の8命令を
`ENABLE_ZBS`で追加した。レジスタ添字は下位6 bit、即値は0〜63を使い、
OP-32／OP-IMM-32の予約encodingは不正命令として扱う。
全Zba/Zbb/Zbsを有効にした構成でのみ、read-onlyの `misa.B` を1にする。

採用構成 `HardwareCompactZbsDspM4KTestTop` は、EXでoperand A/Bを既存レジスタへ
渡し、CMで単一bitの更新／抽出を完了してWBに格納する。
Zbbと同じ依存待ちを使い、共通のCM転送経路へ演算を追加しない。
既存のM/DSP、4 KiB L1I、FAST_LSU=5、分岐予測構成を維持する。

- RTL／ECP5 mappedの独立整数oracle: 各201,728組に合格。
- 無効設定と予約encoding: 262,144通り、外部LLVM assembler: 260 encodingに合格。
- workspace: 194 PASS／既存除外1件。Coreでは依存距離0〜3、両operand、
  若いwriter、メモリ待ち、Zbb/M/CSR混在、redirect／例外、misa.Bの全設定組合せを検証した。
- 自作9 ELF、公式ACT4/Sail 8 ELF、C診断6 ELFを追加し、旧207本と同じSHAを維持した
  **230 ELF × 2 clock順 = 460 PASS**。
- 認定bitの実機: **230種類 + 診断64反復 + 既定起動16実行 = 310 PASS**。
  別途、旧Zbb bitでbase C 3本を各3回実行した。

EXでZbsを完了する試作はsetup −1,586 ps、CMへ移した試作は−397 psだった。
MDU operandレジスタのenable fanout上限8を追加した `zbs-04-cm-ce` は−27 psまで改善した。
Texoで未達の全coneと予測値が同点の配置も探索する試作 `zbs-09-frontier` が収束した。
通常版で合成・物理検査・STAをやり直した `zbs-10-production` と `zbs-11-reproduction` も
setup 0 ps／hold +90 psとなり、checkpointとbitstreamはbyte単位で一致した。
すべてsetup・hold各200 ps予約後の値。検査数は各17,232、JTCK各515、従来の例外4件を維持する。
JTCK照合とchecked bitgenも通過した。依存pinと通常ツールは変更していない。

| 項目 | 認定結果 |
| --- | --- |
| CPU / memory / JTCK | **124.8 / 62.4 / 6 MHz** |
| setup: nominal / 200 ps予約後 | **+200 / 0 ps** |
| hold: nominal / 200 ps予約後 | **+290 / +90 ps** |
| セル / DSP / EBR / PIPs | 29,296 / 16 / 150 / 365,088。Zbb版から676セル増 |
| workspace / 機能モデル / 実機 | **194 PASS・既存除外1件 / 460 PASS / 310 PASS** |

JTCKは同じSTAエンジン・遅延データを使う別構築グラフとの照合であり、独立した遅延characterizationではない。
JTAGG外部境界、reset／CDCと既存4件の例外を維持した。
既存除外1件はmapped board resetのNonScalarControlによるWRITE64試験。
最終bitはWindows D2XX/XVCで書き込み、Veryl、Rust fmt、全target Clippyも通過した。

- 既定image: `build/riscv-hardware-compact-zbs-dsp-m-4k-124p8-20260909.bit`
- bit SHA-256: `40c0243fa152a10fab539fee65406e46d5414e2dbd117a1042231ee5b2577b0a`
- checkpoint SHA-256: `84563e488ee85008244a1c2cbca01dc75e7c40f9920ec3cb5239b744da9d216b`

### 同じC処理の実機時間

Clang 20、`-O2 -mabi=lp64 -mcmodel=medany`で同じ入力と検算を持つCソースを
`rv64imc_zicsr_zifencei_zba_zbb`とZbs追加版へコンパイルした。
base版へのZbs命令混入がなく、追加版で目的の命令が生成されることもdisassemblyで確認した。
以下は同じ認定bit上の3回中央値。起動・検算を含むfreeze済みCSRを使う。

| 同じ仕事 | cycles: base → Zbs | 時間: base → Zbs | 高速化 | 時間短縮 |
| --- | ---: | ---: | ---: | ---: |
| 単一bit更新 | 27,208 → 23,103 | 218.013 → 185.120 µs | **1.177683倍** | **15.087%** |
| bit抽出 | 23,131 → 21,059 | 185.345 → 168.742 µs | **1.098390倍** | **8.958%** |
| 即値操作 | 25,166 → 21,057 | 201.651 → 168.726 µs | **1.195137倍** | **16.328%** |

| 診断 / ISA | retired | IPC | IPC × 124.8 MHz（MIPS） |
| --- | ---: | ---: | ---: |
| mutate / base | 13,390 | 0.492135 | 61.418406 |
| mutate / Zbs | 10,316 | 0.446522 | 55.725958 |
| extract / base | 15,435 | 0.667286 | 83.277333 |
| extract / Zbs | 12,363 | 0.587065 | 73.265701 |
| immediate / base | 15,414 | 0.612493 | 76.439132 |
| immediate / Zbs | 12,339 | 0.585981 | 73.130417 |

IPCが下がっても、命令数の削減で同じ仕事の時間は短くなった。
既存26診断と別途測定したbase C 3本は、旧Zbb bitとの中央値差がすべて±1 cycle以内。
固定4診断の速度比幾何平均は1.000029。乗算は10,147 cycles、2,580 retired、
IPC 0.254262／31.731940 MIPSで維持した。
同時edgeモデルのC 6本＋乗算はminstretが一致し、実機との差は0〜1 cycle、memory waitも一致した。
これは観測条件での結果であり、未測定の全clock位相や外部CDCの保証ではない。

### 次の改善計画

1. **依存転送・LSUを優先する。** B拡張後のCでも依存待ちとメモリ待ちが残る。
   単純なZbb/Zbs演算の早い転送とload依存待ちを個別に試作し、同じELFとC処理で比較する。
   124.8 MHzとsetup／hold各200 ps予約を維持できた案だけを実機評価する。
2. **MDUは除算の待ちを次に評価する。** 乗算はDSP版を維持し、除算の早期終了などを
   実際のコンパイラ生成コードで測る。命令数が変わる比較は処理時間を主指標にする。
3. **Texoの探索改善を整理する。** 最悪cone限定・予測値が厳密に良い候補だけという制限は、
   今回−27 psで停止した。未達の全coneと予測値同点の候補を試す案が収束した。
   保存した差分を基に、汎用の回帰と探索コストを評価する。STAの条件と遅延データは維持する。

A、特権／割り込み、F/D/Vは対象ソフトの要件に合わせた別工程とする。
全32診断のIPC／クロックは [性能表](../toolchain/measurements/2026-09-09-rv64-zbs/PERFORMANCE.md)、
試作・再現・コマンドは [Zbs測定記録](../toolchain/measurements/2026-09-09-rv64-zbs/README.md)へ保存した。
