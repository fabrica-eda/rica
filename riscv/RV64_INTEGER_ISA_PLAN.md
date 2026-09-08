# RV64整数ISAの拡充と検証計画

2026-09-08着手、2026-09-09に第一段階のZbaを実機認定した。
比較基準は `9526c1a` のDSP版、CPU 124.8 MHz／メモリ62.4 MHz、
setup・hold各200 ps予約、133種類のELF／181実機実行。

## 方針と現在地

整数ISAを **I/M/Zca → Zba → Zbb → Zbs** の順に拡充する。
拡張ごとに機能・タイミング・実機を確定し、コンパイラが生成する命令列で比較する。
今回のZbaは完了。Zbb/Zbsを含むB全体の対応は今後の工程である。
仕様は [RISC-V B v1.0](https://docs.riscv.org/reference/isa/unpriv/b-st-ext.html)。

現在の圧縮命令は整数Zca。M-modeの限定したCSR／同期例外も実装している。
A、特権／割り込み、F/D/Vは動かすソフトに合わせて別工程とする。
LSU・MDUの追加高速化は、整数拡張後のコンパイラ生成コードで効果を判断する。

## 採用した構成

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

## 物理条件と検証結果

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

## 実機性能

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

## 次の工程

1. **Zbb**：論理反転、min/max、符号・ゼロ拡張、rotate、count、byte操作を実装する。
   rotate/countの経路が既存ALUを遅くする場合は、完了段を分けて依存制御を明示する。
   独立oracle、全ACT4 catalog、例外／転送、C生成コードを揃える。
2. **Zbs**：単一bit操作と即値版を追加し、同じ検査と物理ゲートを適用する。
   Zba/Zbb/Zbs全体の合格後にB対応を宣言する。
3. **LSU・MDU**：拡張後のCコードを使い、load依存待ちと除算待ちを再測定する。
   同時edgeモデルで見積もり、同一仕事の実機時間で採用する。
4. **ツール側**：Struoの定数arithmetic foldingと、Texoの局所配置探索の改善余地を残す。
   今回は定数の明示と合法な配置変更で認定条件を満たした。STA条件を緩める工程は設けない。

全段階で124.8 MHz、setup・hold各200 ps予約、通常STA＋JTCK照合、同一hashの実機試験を維持する。
結果、コマンド、全診断のIPC／clock、再現用artifactは
[測定記録](../toolchain/measurements/2026-09-08-rv64-zba/README.md)に保存する。
