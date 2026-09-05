# ECP5向けRV64アーキテクチャの検討結果と実装計画

作成日: 2026-09-05。対象: **LFE5UM5G-85F / CABGA381 / speed 8**。

## 必須条件と現在の構成

**CPU 124.8 MHz／メモリ62.4 MHzのPLL設定を必須条件とする。**
配置配線後のsetup/holdが成立しない構成は、サイクル数が少なくても採用しない。
目標周波数を下げて採用条件を満たしたことにはしない。

**単発行インオーダーRV64＋静的順次予測＋4 KiB L1命令キャッシュ**を、
124.8 MHzでSTA合格した開発基盤とする。
Texoの配線後setup余裕は **+0.064 ns**、hold余裕は **+0.290 ns**。
未達状態で採用とした前版の判断は撤回し、固定PLLでの成立を確認し直した。
[124.8 MHzでの改修結果・性能・再現手順](RV64_124P8_TIMING.md)を参照。
命令キャッシュの2/4/8 KiB版、`FENCE.I`、取得・補充・分岐経路の分割を実装した。
`HardwareCached4KTestTop` / `--static-predictor --icache-kib 4` で選択できる。

最終RTLの診断4本は、同じ周波数でのサイクル数比較で幾何平均 **1.470倍**相当。
初期化込みでは **1.417倍**相当。140項目のRustテスト、通常モデル294実行、
ボード相当モデル784実行が合格した。6 KiBの容量検査では4 KiB版は逆に遅くなるため、
汎用プログラムの全てで高速という結論ではない。

合格したTexo実装のbitstreamをWindows D2XX経由で実機へ書き込み、
**124.8 MHz設定で98/98 ELFsと各11個の性能カウンタ検査が合格した**。
ただし、この結果は**1-byte単位のJTAG書込み**を使用する。8-byte書込みでは
payloadの破損を再現したため、ランナーの既定を1-byteへ変更した。
CPUの実行速度を下げる回避策ではないが、ホストからのロード時間は増える。
根本修正とJTAGクロックのタイミング検証は残る。
[実機の測定値と検証条件](../toolchain/measurements/2026-09-05-rv64-124p8-hardware/README.md)を参照。
nextpnrの別配置3件は未達であり、採用する実装は上記のTexo checkpointに限定する。
[以前のL1I比較](RV64_L1I_IMPLEMENTATION.md)の1.575倍とFmax中央値98.814 MHzは
タイミング改修前の参考記録であり、現在の実機性能ではない。
下記の予測器比較の数値は、L1I導入前の初回計測として保持する。

## 初回の予測器比較（L1I導入前）

基本構成は、**単発行・インオーダーのRV64パイプライン、同期BRAMレジスタファイル、
EXバイパス**を採用する。今回、既存TAGE構成に加えて、bimodal構成と、
**静的な順次実行予測＋次PCが誤った場合だけ回復する最小構成**を実装した。
後者は `HardwareStaticTestTop` / `--static-predictor` で選択できる。

今回の4本の診断ワークロードを等しく重視するなら、最小構成を暫定候補とする。
起動初期化を除いたボード相当モデルと配線後Fmax中央値から計算した推定処理速度は、
TAGE比で幾何平均 **1.031倍**。LUT4は **18,987 → 11,192（41.1%減）**、
FFは9,160 → 4,606。bimodalは同じ集計で0.964倍であり、全面的な置き換えは選ばない。

ただし、最小構成の `load_store` はTAGEより推定実行時間が9.4%長い。
約3%の平均差は配置seedによる変動も考慮すべき差であり、汎用プログラムでの最速を
証明した結果ではない。既存の既定値はTAGEを維持し、新構成は明示的に選択する。
大規模アプリケーション、他のRV64コア、2命令発行、OoOとの実測比較は行っていない。

**3構成とも現在のPLL設定124.8 MHzではタイミング未達**。
RTLの正しさとアーキテクチャ比較までは完了したが、タイミングを満たす動作周波数の
確定、bitstreamの実機検証、デバイス上の最速構成の確定は未完了である。

## 初回比較で定めた改良方針

**今後の開発基盤には、単発行インオーダー＋静的順次予測の軽量構成を推奨する。**
判断の軸は、今回の平均性能をおおむね維持しながら、LUTを41.1%、FFを49.7%減らし、
命令供給や分岐処理の改善を加える土台を単純にできること。
4本の診断テストでの約3%差だけで、汎用性能の優劣を確定する判断ではない。
TAGEは精度と実行時間の比較基準として保持する。今回のbimodalは主要な開発対象に選ばない。
この採用推奨の追記によってRTLの既定値を変更したわけではなく、現時点では
`HardwareStaticTestTop` / `--static-predictor` で軽量構成を選ぶ。

改良は次の順序を基本とする。CPU内の配線改善は、命令供給改善と並行して進められる。

| 改良 | 現在の根拠と期待する効果 | 見積りの限界 |
| --- | --- | --- |
| 同期L1命令キャッシュ（2/4/8 KiBを比較） | 再利用用バッファが現在16バイト。ループの命令再取得を減らす | 命令取得待ちがどこまで減るかを実装して測る。容量の追加だけで一定倍率になるわけではない |
| データアクセスの待ち時間短縮 | 静的構成のload/store依存が今回の弱点。要求/ACKの遅延を分類し、ポート構成やデータキャッシュを比較 | 命令キャッシュだけではデータ待ちを除けない。副作用・整合性を含めて検証する |
| bypass/redirect経路の局所化と段構成の調整 | 静的構成の律速経路にも配線遅延8.204 ns。CPU Fmaxの改善対象が残っている | 段追加による分岐・依存命令のCPI増を含めて時間で判断する |
| 小型の分岐先・関数復帰予測 | 命令供給改善後にtaken分岐やreturnの待ちを減らす。TAGEと新しい小型構成を比較 | 現在のbimodalはBTB/RASと管理回路を共有しており、予測器全体を小さく作り直す案とは別物 |
| 2命令発行、さらにOoO | 単発行の上限を超える選択肢 | 命令供給、RFポート、バイパス、発行・回復を再設計する規模。追加面積だけで速度が上がるとは判断できない |

改良幅を判断する参考として、静的構成の依存ADDIループは、ボード相当の実行区間が
11,783クロック、即時メモリの通常モデルが6,151クロックで約1.92倍の差がある。
他の診断ループも約1.72〜2.50倍の差があるため、メモリ接続を含む命令・データ供給に
改善余地がある。ただし両モデルは測定端点とメモリ条件が異なり、
**この比率をL1Iだけの高速化倍率や到達可能な性能上限とは扱わない**。

周波数については、配線後推定95.447 MHzから124.8 MHzが成立し、CPIを保てるなら、
計算上の速度比は約1.31倍。ただし目標周波数は未達であり、キャッシュや段追加によって
周波数とCPIの両方が変わる。メモリ改善の参考比率と、この1.31倍を単純に掛け合わせない。

資源面では、静的構成のEBRは136/208個で72個残る。ただしRAM幅やポート数による
分割と配置制約があるため、残数をそのままキャッシュの有効容量には換算しない。
命令キャッシュ導入後は分岐ミスの相対的な重みが変わり、予測器の順位が逆転する
可能性もあるため、**単発行の実行系を基盤にしつつ、予測方式は再選定する**。

## デバイス、ISA、評価範囲

- ECP5-85は約84K LUT、208個の18 Kbit EBR、156個の18×18乗算器を持つ。
  同期RAMと局所的な配線を使う構成を優先する。
  [Lattice公式仕様](https://www.latticesemi.com/en/Products/FPGAandCPLD/ECP5.aspx)
- ボードは12 MHz基準、PLL VCO 624 MHz、CPU/メモリ分周5/10。
  実クロック設定は124.8/62.4 MHz。256 KiB統合BRAM、JTAG、CSR、デバッグ回路も
  含めたボード全体を合成・配置配線した。
- XLEN=64、32本の整数レジスタ、64ビットALU/ロード・ストア、RV64のW命令の
  符号拡張を維持する。物理アドレスは既存プラットフォームの32ビット幅。
  [RISC-V公式RV64I仕様](https://docs.riscv.org/reference/isa/v20240411/unpriv/rv64.html)
- ISA範囲は既存の**RV64I＋整数圧縮命令Zca＋machine CSRの実装範囲**。
  M/A/F/D/V、MMU、Linux対応を追加した実装ではない。
- ワークロードの追加指定がなかったため、既存の混合命令トレース、8本のELF、
  ACT4 I/Zcaと、新規の4本の診断ループを用いた。ACT4は正しさの検査であり、
  アプリケーション性能の点数として扱わない。
- 作業開始時にCore、BranchPredictor、InstructionBufferと関連テストにあった
  未コミット変更を保持し、それを比較の出発点にした。
  過去のnextpnr 0.11.1による記録とはRTL・ツール条件が異なるため直接比較しない。

## アーキテクチャの比較と判断

| 候補 | 利点 | このデバイスで確認すべきコスト | 判断 |
| --- | --- | --- | --- |
| 非重複マルチサイクル | 各段を短くできる | 高FmaxでもCPIが大きい。例: 250 MHz/CPI 15は16.7 M命令/s | 基本構成に選ばない |
| 単発行＋TAGE/BTB/RAS | 分岐相関を学習できる | タグRAM、履歴、回復、広い配線 | 既存基準を維持 |
| 単発行＋bimodal/BTB/RAS | タグRAMを省き、基底カウンタで予測 | BTB/RASと管理状態は残り、相関分岐で精度低下 | 実装・測定済み。平均では採用しない |
| 単発行＋静的順次予測 | 予測RAM・履歴・RASを除去し、応答を短縮 | taken分岐や関数復帰で再取得が増える | 実装・測定済み。今回の暫定候補 |
| 2命令発行インオーダー | 独立命令を並列実行 | 4R/2W相当のRF、追加バイパス、発行ハザード、2命令の境界判定、retire幅 | 命令供給改善後に再評価 |
| Out-of-Order | 独立演算と長いロード待ちを重ねる | rename、ROB、issue、LSQ、精密例外、比較と配線 | 現在のBRAM中心・単一スレッド用途では後順位 |
| 複数コア・SMT | 並列処理の総量を増やす | メモリ帯域共有、並列ソフトウェアが必要 | 単一スレッドの評価には含めない |

複数発行で増えるRFポートやバイパスの例は、BOOMの公式設計資料にも示されている。
これをECP5上の面積・配線上の懸念としたのは設計上の推論であり、BOOMとの速度比較ではない。
[BOOM Register Files and Bypass Network](https://docs.boom-core.org/en/latest/sections/reg-file-bypass-network.html)

既存のパイプラインは、取得、展開・デコード、同期RF読み出し、EX1、完了/EX2、
書き戻しを重ねる。連続する依存ALU命令はEXバイパスにより1命令/クロック、
ロード・シフト・比較依存は結果生成まで待つ。この実行系を共通にして予測方式を比較した。

重要な制約は命令供給である。現在の命令ポートは64ビットをCPUの半速で取得するため、
バッファに再利用がない32ビット命令列では、転送量だけでも理想上限は
`64 / 32 / 2 = 1命令/CPUクロック`。要求・応答の待ちで実効値はさらに下がる。
2本の8バイト行しか保持しないInstructionBufferを超えるループでは再取得が発生する。
発行幅を2にする前に、同期L1命令キャッシュと再取得の重なりを改善する優先度が高い。
圧縮命令やキャッシュヒット時まで、この転送量の上限を一般化するものではない。

## 実装内容

| 構成 | Core `BRANCH_PREDICTOR_CONFIG` | ボード/メモリtest top `PREDICTOR_MODE` | CLI | 合成入口 |
| --- | --- | --- | --- | --- |
| TAGE（既定） | 0、reset scrubありは1 | 0 | 指定なし | `HardwareTestTop` |
| bimodal | 2、scrubありは3 | 1 | `--bimodal` | `HardwareBimodalTestTop` |
| 静的順次予測 | 4、5も同じ動作 | 2 | `--static-predictor` | `HardwareStaticTestTop` |

Coreの設定bit 0はscrub、bit 1はbimodal、bit 2は静的予測を指定する。
ボードwrapperは動的予測器のscrubを有効にする。
以前のCoreパラメータ `SCRUB_BRANCH_PREDICTOR` はこの設定に置き換え、リポジトリ内の
全呼び出しを更新した。独自の外部wrapperがあれば同様の移行が必要になる。

- **bimodal**: BranchPredictor/TageDirectionPredictorの `USE_TAGE=0` で、
  4本のタグ付きRAMをgenerateから除外し、タグ応答・割当・学習・agingを止める。
  2048×2ビット基底テーブル、128セット×2-way BTB、16段RAS、retire token、
  同じ行の連続更新とA/B/A更新のforwardingは共有する。
  マップ上には履歴FFが216個残るため、履歴回路まで完全に除去した構成ではない。
- **静的予測**: `StaticBranchPredictor.veryl` は1エントリの応答レジスタに
  fallthrough PCを保持する。PC+2/PC+4、backpressure、同時消費・置換、回復時の破棄に対応。
  学習状態がないため、Coreは次PCが正しい場合に型・方向情報の修復だけでflushしない。
  次PCが誤った場合、例外、メモリ待ち、誤経路のストア/CSR抑止は既存パイプラインで処理する。
- **評価系**: ELFランナーは構成ごとにモデルを1回コンパイルし、ELFごとに新しい状態を作る。
  前のELFの学習結果が次の測定に残らない。4本の自己検査付きRV64ループを追加した。
- **物理比較**: 同じデバイス、LPF、PLL、placer、router、seedを使う
  [比較スクリプト](../toolchain/measurements/run_rv64_architecture_ab.py)を追加した。
  ボードwrapperの階層をCSRのCE制約・クロック判定にも適用する。
  nextpnr 0.6のpacked CLKピン名ではなく、clk-to-qのQ側から発信レジスタのドメインを判定する。

## 測定条件と結果

Veryl 0.20.3、Cargo.lock固定のStruo/Celox、nextpnr-ecp5 **0.6-3build5**、
heap placer / router1 / seeds **1, 2, 5**。最終RTLでTAGE/bimodalを再合成し、
採用したマップとのバイト一致も確認した。
[機械可読の結果・SHA-256](../toolchain/measurements/2026-09-05-rv64-architecture/summary.json)に
RTL、ELF、マップ、配置配線結果のハッシュを記録した。同じディレクトリに9本の
タイミングreportと検証ログを保存してある。途中の試作値は以下の表に混ぜていない。

### 回路規模と配置配線

| 構成 | LUT4 | FF | EBR（DP16KD） | 全マップセル数 |
| --- | ---: | ---: | ---: | ---: |
| TAGE | 18,987 | 9,160 | 150 | 31,335 |
| bimodal | 18,060 | 8,982 | 146 | 30,084 |
| 静的順次予測 | 11,192 | 4,606 | 136 | 18,060 |

LUT4列はPFUMX等を含めた配置後slice占有率ではない。全構成に256 KiBメモリを含む。
Struoのmapping等価性検査は3構成とも通過。静的構成ではretimingが適用された。

| 構成 | CPU Fmax: seed 1 / 2 / 5 (MHz) | 中央値 (MHz) | CPU→mem最大 (ns) | mem→CPU最大 (ns) | 現PLLのsetup検査 |
| --- | --- | ---: | ---: | ---: | --- |
| TAGE | 84.296 / 84.027 / 88.292 | 84.296 | 3.172 | 7.603 | 0/3合格 |
| bimodal | 91.937 / 84.998 / 89.262 | 89.262 | 3.381 | 7.537 | 0/3合格 |
| 静的順次予測 | 95.447 / 95.247 / 96.777 | 95.447 | 3.022 | 7.794 | 0/3合格 |

今回の9本はCPU内部のsetupが未達。CPU↔memoryの両境界は約8.013 nsの制約内だった。
CPU Fmaxだけでなく、メモリFmax、2:1の周波数比、両方向の経路を検査した。
nextpnr reportの制約値124.8128/62.4025 MHzは期間丸めによる表記で、PLL設定は124.8/62.4 MHz。

中央値seedのTAGEではタグRAM→予測結果の経路が律速で、配線部分が9.230 ns。
静的構成でも完了段の選択→redirect targetの経路に8.204 nsの配線遅延が残った。
予測器を省くだけで目標周波数まで到達する結果にはならなかった。

### 実行サイクルと推定時間

[診断プログラムの内容](tests/rv64-benchmarks/README.md):
`alu_chain` は依存ADDI、`branch_pattern` はN,N,T,T分岐、`call_return` は圧縮call/return、
`load_store` は同じ1ワードへのload-use/store依存。配列帯域・キャッシュ容量の測定ではない。

| プログラム | 命令数（通常モデル） | 通常モデル cycles: TAGE / bimodal / 静的 | ボード相当・初期化込み cycles: TAGE / bimodal / 静的 |
| --- | ---: | --- | --- |
| alu_chain | 4,614 | 5,671 / 5,671 / 6,151 | 14,373 / 14,373 / 11,793 |
| branch_pattern | 2,823 | 6,226 / 8,741 / 7,432 | 13,519 / 18,743 / 12,819 |
| call_return | 1,544 | 4,656 / 4,656 / 5,641 | 11,837 / 11,837 / 10,771 |
| load_store | 1,288 | 3,108 / 3,108 / 3,081 | 8,262 / 8,262 / 7,699 |

通常モデルは命令/データの即時アクセスで、terminal store要求までを数える。
ボード相当モデルは2:1クロック、InstructionBuffer、メモリACKを含み、terminal storeの
commitまでを数えるので命令数が1多い。両モデルのIPCを同じ条件の値として混ぜない。
ボード相当の数値は同時エッジの両処理順序のうちサイクル数が大きい方。

動的予測器には2048クロックのreset scrubがあり、最初のcommitは2057クロック目。
静的構成は10/11クロック目なので、実行そのものの比較には**最初のcommitから最後の
commitまでの両端を含む区間**を使う。初期化・最初の取得遅延は除くが、予測器の事前学習や
キャッシュのウォームアップは行わない。

| プログラム | 実行区間 cycles: TAGE / bimodal / 静的 | 推定時間 μs: TAGE / bimodal / 静的 |
| --- | --- | --- |
| alu_chain | 12,317 / 12,317 / 11,783 | 146.117 / 137.987 / 123.450 |
| branch_pattern | 11,463 / 16,687 / 12,809 | 135.986 / 186.944 / 134.200 |
| call_return | 9,781 / 9,781 / 10,761 | 116.032 / 109.577 / 112.743 |
| load_store | 6,206 / 6,206 / 7,689 | 73.622 / 69.526 / 80.558 |

`推定時間[μs] = cycles / 推定周波数[MHz]`。
推定周波数は `min(CPU Fmax, 2×memory Fmax, 1000/max(両境界遅延ns))` のseed中央値で、
今回はCPU Fmaxが制限している。**これは同じ2:1比で周波数を変えられると仮定したSTA上の
比較値**であり、95.447 MHz等をPLLに設定して動作検証した結果ではない。
実際の周波数を選んだ後には、そのPLL・制約で再配置配線とsetup/hold検査が必要。

| TAGE比の推定処理速度・4本の幾何平均 | bimodal | 静的順次予測 |
| --- | ---: | ---: |
| 通常モデル | 0.973倍 | 1.014倍 |
| ボード相当・初期化込み | 0.976倍 | 1.256倍 |
| ボード相当・初期化を除く実行区間 | 0.964倍 | 1.031倍 |

初期化込みの1.256倍を定常実行性能の改善率とは扱わない。
周波数だけが高いbimodalも、相関分岐の追加サイクルを取り戻せなかった。

### 正しさの検証

- Veryl check/fmt、Rust fmt、差分の空白チェック: 合格。
- Rustテスト: **125項目合格**。123項目の全体回帰後、追加・変更したtiming guardを9項目、
  Core回復を19項目で再検証した。静的構成の正しいfallthroughでの無flush、メモリ待ち中の
  回復/例外、誤経路store/CSR抑止、backpressure/PC+2/flushを含む。
- **各構成**: 既存ELF 8/8、ACT4 RV64I 51/51、Zca 32/32、診断ループ4/4合格。
- **各構成**: ボード相当の既存ELF＋ACT4を両エッジ順で182/182、診断ループを8/8合格。
- 混合58命令トレース: TAGE/bimodalは126クロック、静的は105クロック。
  単純なボード診断ループはTAGE/bimodal 24,603、静的32,771クロック。
  ここでも短いトレースだけでは構成の優劣を判断できない。
- bitstreamの生成・書き込みと実機上のISA/性能検証: **未実施**。
  nextpnr 0.6の今回のreportだけでholdを含む実機動作保証を出さない。

## 実装計画と完了条件

| 段階 | 内容・主な変更箇所 | 完了条件 | 状態 |
| --- | --- | --- | --- |
| 1. 共通基準 | 作業開始時RTL保存、RV64診断ELF、区間計測、同一P&R比較 | 同じELF・3 seeds・再現できるログ/ハッシュ | 完了 |
| 2. 構成選択 | Core、TageDirectionPredictor、StaticBranchPredictor、各test top | ISA/回復回帰と3構成の面積・時間比較 | 完了 |
| 3. 命令供給 | 同期L1Iの2/4/8 KiB、64-bit行、次行先読み、補充連続要求、FENCE.I | ヒット時1命令/clock、PC+2と跨行命令、miss中redirect、コード更新・再ロードの整合性 | 完了。4 KiBを採用 |
| 3b. データ供給 | LSU要求/応答とload-useを計測し、ポート接続や小型L1Dを比較 | store順序とFENCE.Iを保ち、実行時間を短縮 | 次の優先実装 |
| 4. CPU内の配線 | 取得PC、EBR取得/補充、completion/bypass→JALR/redirectの分割と局所化 | 固定PLLで内部・関連クロックのsetup/holdが成立し、追加段の待ちを含めて性能評価 | 完了。setup +64 ps / hold +290 ps |
| 5. 採用構成・動作周波数 | cache容量・予測方式・段数を比較し、適合した配置配線結果を固定 | CPU 124.8 MHz／メモリ62.4 MHzでsetup/holdを満たし、用途ごとの時間と面積を比較 | 4 KiB・静的予測のTexo実装で固定PLL合格 |
| 6. 実機 | 固定PLL/bitstreamでJTAGロード、ELF/ACT4、counterの読出し | 全ELFとcounter検査の通過、実行時間を記録 | 1-byte書込みで98/98・各11カウンタ合格。実機値を記録済 |
| 6b. JTAG転送修正 | 8-byte payloadの読戻しと段ごとの照合、JTAG setup/hold制約、モデル連携の修正 | 8-byte転送の再現診断と98 ELFsを修正bitstreamで通す | 未完了。1-byte書込みを既定として回避 |
| 7. 発行幅拡張 | 上記で供給余力と独立命令が確認できた場合だけ2命令発行を試作 | RF/バイパス増加を含めても単発行より実行時間短縮 | 条件付き |

L1Iのコード書換え・デバッグ再ロード時の整合性は実装・検証済み。
2/4/8 KiBとも追加10 EBR、採用構成は146 EBRとなった。今後のRAM追加も実マッピングで確認する。
OoOへ進む場合は、既存の単一のretire frontierを流用せず、分岐ごとのcheckpointと
メモリ/例外の精密な回復を新しく設計する。

## 初回比較の再現手順

L1I導入後の再現コマンドは [L1I実装結果](RV64_L1I_IMPLEMENTATION.md#再現手順)を参照。

リポジトリのルートから実行する。ACT4 ELFの生成手順は既存の
[README](README.md)に従う。今回使ったELFのハッシュは結果JSONに保存してある。

```sh
cargo test --locked --offline -p verify-riscv
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --benchmarks --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --bimodal --benchmarks --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --static-predictor --benchmarks --max-cycles 100000
cargo run --locked --offline -p verify-riscv --bin verify-riscv -- --static-predictor --hardware-memory-rtl riscv/build/rv64-benchmarks/*.elf
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --static-predictor --act4
cargo run --locked --offline -p verify-riscv --bin rv64-riscv-tests -- --static-predictor --act4-suite Zca
```

ハードウェアメモリ/ACT4のコマンドも、予測器フラグを外す・`--bimodal`に置き換えることで
他の構成を確認できる。物理比較は同じnextpnrバイナリを使い、空の出力ディレクトリを指定する。

```sh
cargo build --locked --offline --release -p verify-riscv --bin nextpnr-riscv
target/release/nextpnr-riscv HardwareTestTop
target/release/nextpnr-riscv HardwareBimodalTestTop
target/release/nextpnr-riscv HardwareStaticTestTop
python3 toolchain/measurements/run_rv64_architecture_ab.py \
  --tage riscv/build/riscv-hardware-test-125.nextpnr.json \
  --bimodal riscv/build/riscv-hardware-bimodal-test-125.nextpnr.json \
  --static riscv/build/riscv-hardware-static-test-125.nextpnr.json \
  --seeds 1 2 5 --output /tmp/rica-rv64-architecture-new-comparison
```

比較スクリプトはタイミング未達でもreportを回収するため `--timing-allow-fail` を使う。
スクリプトの正常終了はタイミング合格を意味しない。結果JSONの `target_timing_pass` と
検査メッセージを確認する。bitstream生成やボード書き込みは行わない。
