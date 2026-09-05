# 小型分岐予測器の実装と評価

2026-09-06。基準は `672dea0` の静的予測＋4 KiB L1I、実機測定記録は `1a50ecb`。
対象は LFE5UM5G-85F / CABGA381 / speed 8。CPU 124.8 MHz、メモリ 62.4 MHz を維持する。

**64エントリBTB＋2-bit方向予測＋8段RASを採用する。**
実機100/100 ELFsが合格し、同じ124.8 MHzで既存4診断の処理率は
幾何平均 **1.2228倍（約22.3%向上）**。各3回の実機測定の中央値で比較した。

更新: その後の[JTAG原因調査と修正](RV64_JTAG_FIX.md)で受信エッジの競合を修正した。
現行の既定は `20260906-jtag-falling.bit`、8-byte書込み、実機100 ELFs／134実行合格。
CPU／メモリsetup +9 ps、hold +290 psで、性能は本稿の認定版から0～1 cycleの差。
以下のCE版の面積・タイミング・測定値は、予測器導入時の記録として保持する。

## 実装

`CompactBranchPredictor.veryl` に、64エントリの直接対応BTB、エントリごとの2-bit方向カウンタ、
8段の復帰アドレススタック（RAS）を追加した。32エントリ版と、RAS／方向予測をそれぞれ
無効にする比較用設定も用意した。退役順序は単発行インオーダーを維持する。

BTBは同期RAM読出しと登録したタグ・種別判定の2段。順次要求は毎cycle受け付けられる。
taken応答を採用したら、すでに受け付けていた若い順次要求を取り消す。
保持中の応答、8個の予測トークン、同一行の読書き転送、reset時の全行初期化を扱う。
エントリは64行版で63 bit、32行版で64 bit。退役時に学習し、方向カウンタには予測時の
スナップショットを使い、直前に退役した同じPCのカウンタを転送する。

RASは投機状態と退役済み状態の2組を持つ。あふれたら最古の復帰先を捨て、空ならBTBの
行き先を使う。x1/x5によるcall／return／coroutineの分類と、圧縮callのPC+2を扱う。
回復時には同じedgeの退役も反映してから投機状態を戻す。

小型モードの予測修復はCM段で行う。EXでは実行先・fallthroughとのPC一致をそれぞれ
登録し、CMで実際の分岐方向に応じて選ぶ。JALRの加算結果から32-bit比較、さらに全体の
制御信号までを1cycleで通る経路を避けた。例外、FENCE.I、待機中のload/store、CSRの
副作用が予測修復で失われないことを回帰試験で確認する。

## 比較用コマンド

```sh
cargo build --release --bin verify-riscv --bin rv64-riscv-tests
target/release/rv64-riscv-tests --predictor-tests --compact-predictor --max-cycles 100000
target/release/verify-riscv --hardware-memory-rtl --compact-predictor --icache-kib 4 riscv/build/rv64-benchmarks/*.elf
```

`--compact-predictor-32`、`--compact-predictor-no-ras`、
`--compact-predictor-no-direction` で比較する。
ボード用topは `HardwareCompact4KTestTop`、32行版は `HardwareCompact32Cached4KTestTop`。
PLLとピン制約は従来の `pll-12-to-124p8.json` と `lfe5um5g-85f-evn.lpf` を使う。

## 固定クロックでの成立条件

合成対象は `HardwareCompact4KTestTop`。CPU **124.8 MHz**／メモリ **62.4 MHz**、
2:1・位相0のPLL設定を維持した。Texo 0.1.6の**合成最適化目標175 MHz**は
論理の選び方を制御する指定であり、CPUを175 MHzで動かす設定ではない。
配置重み指数は3。既存のCE fanout制約に、小型予測器の登録段の制約を追加した。
退役からトークン空き判定への同一cycleバイパスを除き、解放したトークンは次cycleから
再利用する。8個でスカラパイプラインを収容でき、測定した全モデル結果は変わらなかった。
命令キャッシュの次行PC計算も4-bit単位へ分け、組合せキャリー経路を短くした。

| 配線後の項目 | 静的4 KiB基準 | 小型64行＋RAS・CE制約版 |
| --- | ---: | ---: |
| CPU / memory MHz | 124.8 / 62.4 | 124.8 / 62.4 |
| setup最悪余裕 | +64 ps | **+11 ps** |
| hold最悪余裕 | +290 ps | **+290 ps** |
| setup / hold検査数 | 各12,250 | 各13,902 |
| LUT4 | 11,138 | 12,689 |
| carry_slice | 1,254 | 2,000 |
| FF | 4,685 | 5,598 |
| EBR | 146 | 150 |

BTBは4 EBRに対応した。carry_sliceは専用算術プリミティブの数で、LUT4数とは別項目。
14,422モデル化端点のうち520端点は従来と同じJTAG／CDCの対象外であり、
**端点名と理由の集合が基準版と一致**することも検査した。
内訳はJTAGクロック未制約519件と、request toggleのCDC初段に同期launchがない1件。
CPU／メモリのsetup/hold余裕をJTAG領域の保証に流用しない。

setupの11 psは合格だが余裕は小さい。クロック増加や回路追加の余地を示す値ではなく、
次の改良でも配置配線後の検査と同じイメージの実機確認が必要になる。
[採用実装のSTA・コマンド・ハッシュ](../toolchain/measurements/2026-09-06-rv64-compact-predictor/timing/ce/metadata.json)
と[試行一覧](../toolchain/measurements/2026-09-06-rv64-compact-predictor/pnr-trials.json)を保存した。

同じRTLの別配置ではsetup +14 psだったが、実機のJTAG SELECT応答がゼロとなった。
従来版への書戻しでは正常動作し、上記CE制約版では全100 ELFsが合格した。
このため+14 ps版は採用対象から外した。後続調査で受信シフタのエッジ関係に絞り、
[JTAG修正](RV64_JTAG_FIX.md)で復旧を確認した。CPUのSTA合格だけでは実機認定としない。
追加の[STA監査](RV64_STA_AUDIT.md)で、未検査端点を許すbitgen条件、JTAGGアークと
reset recovery/removalの検査不足、setup uncertaintyが0であることを確認した。
本稿のSTA合格は検査済みCPU／メモリ範囲に限り、全ドメインのタイミング認定を意味しない。

## モデルによる構成比較

RAM読出しと判定を分割した版のモデル値は以下。最初から最後のcommitまでの区間であり、
起動から終端store応答までを含む実機カウンタとは異なる。

| 診断 | 静的版 cycles | 2段lookup版 cycles | 速度比 |
| --- | ---: | ---: | ---: |
| alu_chain | 6,712 | 5,715 | 1.174 |
| branch_pattern | 8,988 | 8,367 | 1.074 |
| call_return | 7,192 | 4,671 | 1.540 |
| load_store | 6,166 | 4,659 | 1.323 |

4本の速度比の幾何平均は **1.266倍**。これはモデルでの実行区間の値であり、
以下の実機全区間の性能と区別する。短い診断の結果を汎用アプリケーション全体の
性能改善率とは扱わない。

追加診断 `rv64-predictor/` は既存4本の集計に混ぜない。

| 追加診断（cycles） | 静的 | 64行＋RAS | 32行＋RAS | 64行・RASなし |
| --- | ---: | ---: | ---: | ---: |
| btb_conflict | 4,928 | 3,179 | 5,723 | 3,179 |
| call_nested | 25,144 | 17,013 | 17,013 | 20,077 |

`btb_conflict` は32行表で衝突する配置を意図的に作った診断で、64行版が32行版の1.800倍。
`call_nested` は複数の呼び出し元を持つ入れ子関数で、RASありはなしの1.180倍だった。
既存4診断だけではこれらの差を観測できないため、64行＋RASを実機評価対象に選んだ。
方向予測を無効にすると既存4本の幾何平均は1.018倍に下がり、条件分岐の学習も必要だった。

容量超過や頻繁なFENCE.Iへの効果は小さく、`capacity_3k` は8,580→8,583 cycles、
`capacity_6k` は20,184→20,203、`fence_i` は1,490→1,519だった。
分岐予測だけでは命令キャッシュ補充の待ちを解消できない。

全体を再計測する場合は、既存98 ELFsと追加2 ELFsをビルドしたうえで、次を実行する。
出力先は存在しないディレクトリにし、過去の測定を上書きしない。

```sh
python3 riscv/tools/evaluate_compact_predictor.py /tmp/rv64-compact-models-new
```

モデル実行のコマンド・RTL/制約・ELF・実行ファイル・ログのハッシュを保存する。
ボードメモリモデルはCPU/メモリ同時edgeの2通りの処理順を必ず比較する。

## 実機評価

**100/100 ELFs合格。以下は各イメージ3回の実機診断の中央値。**
Windows D2XX → XVC（127.0.0.1:3721、JTAG 6 MHz）で同じボードを使用し、
IDCODE `0x81113043` を確認した。ロードは検証済みの1-byte書込みを使う。
従来版と候補版を実際に書き換えて測り、ELFのバイト列は固定した。

実機の `mcycle` / `minstret` は起動・初期化から終端storeの応答までを含み、
JTAGロード時間は含まない。`IPC × 124.8` の単位は百万退役命令/秒。
クロック値はPLL設定であり、外部計測器による独立測定値ではない。

| 診断 | 静的mcycle | 小型mcycle | minstret（共通） | 小型IPC | 小型IPC × MHz | 速度比 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| alu_chain | 6,989 | 5,994 | 4,616 | 0.770103 | 96.109 | 1.166 |
| branch_pattern | 9,265 | 8,644 | 2,825 | 0.326816 | 40.787 | 1.072 |
| call_return | 7,472 | 4,946 | 1,546 | 0.312576 | 39.009 | 1.511 |
| load_store | 6,451 | 5,448 | 1,290 | 0.236784 | 29.551 | 1.184 |
| capacity_3k | 8,865 | 8,860 | 6,174 | 0.696840 | 86.966 | 1.001 |
| capacity_6k | 20,509 | 20,483 | 6,169 | 0.301177 | 37.587 | 1.001 |
| fence_i | 1,811 | 1,824 | 72 | 0.039474 | 4.926 | 0.993 |
| btb_conflict | 5,220 | 3,461 | 1,036 | 0.299335 | 37.357 | 1.508 |
| call_nested | 26,453 | 19,329 | 5,135 | 0.265663 | 33.155 | 1.369 |

既存4診断の速度比の幾何平均は **1.2228倍（約22.3%向上）**。
容量診断はほぼ同等、FENCE.I診断は約0.7%遅い。追加2本を4本の平均へ混ぜない。
各診断の3回の `mcycle` は最大値−最小値が0または1 cycleだった。
[全反復値・中央値・速度比](../toolchain/measurements/2026-09-06-rv64-compact-predictor/summary.json)を保存した。
`performance.elf` はカウンタへ既知値を書き込む機能試験なのでIPC集計から除外する。

モデルの1.266倍を実機の改善率とは呼ばない。特に `load_store` の実機は
中央値5,448 cyclesで、モデルの最初〜最後のcommit区間4,659 cyclesとは異なる。
実機のメモリ待ちと起動を含むカウンタ同士の比較を、採用性能の判断に使う。

## 検証と再現

- Rust回帰: **150項目合格、既存の1項目はignore**。小型予測器単体8項目とCore回復24項目を含む。
- 最終RTLの通常Coreモデル: **100/100 ELFs合格**。
- 同じRTLのボードメモリモデル: **100 ELFs × 2エッジ順序 = 200/200合格**。
- 32行／RASなし／方向予測なし／静的版: 各9診断 × 2エッジ順序が合格。
- Veryl check/fmt、Rust fmt: 合格。
- 実機: **100/100 ELFs合格**。基本8本＋既存4診断＋キャッシュ3本＋追加2本＋ACT4 RV64I 51本＋Zca 32本。
- 旧版・新版とも診断9本を3回ずつ実行し、各ELFで11カウンタと `tohost=1` を確認。
- 新しい既定設定によるSRAM再書込み後も基本8本が合格。新版は反復を含む**計127実行**が合格。

最終モデルの証跡は `models-token/`。以前の `models/`、`models-cm-ras/`、
`models-path-match/` は途中のRTLの記録であり、最終のソースハッシュと取り違えない。
[モデル記録](../toolchain/measurements/2026-09-06-rv64-compact-predictor/models-token/source-sha256.json)と
[検査ログ](../toolchain/measurements/2026-09-06-rv64-compact-predictor/verification/metadata.json)を参照。

採用する実機イメージは次のハッシュに限定する。ランナーの既定もこのイメージと1-byte書込みへ更新した。

- bitstream: `build/riscv-hardware-compact-4k-124p8-20260906-ce.bit`
- SHA-256: `ed8b12372736055d799bbb6b515ad7f73bad5bb4785b093e7e5cf4be41017deb`
- checkpoint: `build/compact-predictor/compact64-token-next-ce.texo.json`
- SHA-256: `2a25a6f5e725c3bd5b4dac2a10af5587d87e42917df753ac6dc5cb2f32b55a6b`

再配置配線には `timing/ce/pnr-command.json` の `command` を使う。
再生成した結果でも固定PLLのsetup/holdと検査対象を確認してからbitgen・実機評価する。
実機記録スクリプトは既存の結果とELFハッシュを保護する。

```sh
python3 toolchain/measurements/2026-09-06-rv64-compact-predictor/run_hardware.py diagnostics \
  --image compact-ce --program --label new-comparison
```

## 次の改良

1. **タイミング余裕とJTAGの再現性**。11 psしか余裕がないため、登録段・CE配線を局所化する。
   未制約JTAGの原因調査は続け、新しい配置ごとにbyte転送と全命令試験を通す。
2. **LSUの待ち短縮**。小型予測器の `load_store` でもmemoryイベントは中央値2,826回残る。
   退役済みstoreのバッファと同一アドレス転送を、順序・例外・FENCE.Iを維持して比較する。
3. **方向予測の改善**。`branch_pattern` は方向誤り385/1,025回が残る。
   短い履歴を使う方式を比較し、lookup段数と124.8 MHzの維持も評価する。
4. **命令補充・容量・FENCE.I**。6 KiBコードでは今回の予測器による効果は小さい。
   連続補充、8 KiB構成、世代タグによる無効化を別々に測る。

イベントは退役と重なることがあるため、上記回数をそのまま削減可能cycleとは見なさない。
実用ワークロードを追加し、診断だけでの選択が妥当か確認する。
