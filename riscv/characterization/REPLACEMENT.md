# 実測配置モデルのネイティブ組み込み

この文書は配置モデルだけを置き換えた前段階の記録。
2026-09-23に [STA表全体の置換](STA_REPLACEMENT.md)を完了し、
現在の既定プロファイルは [qualified-measured-sta.json](../qualified-measured-sta.json)。
以下の旧プロファイル・旧STAの結果は比較元として保持する。

2026-09-22。実測ROモデルをTexo本体の配置改善へ組み込む。
`apply_model.py` による外部の候補生成から、Rustの候補生成・配線・全体STA・
採否判定へ置き換えた。実行記録は
`toolchain/measurements/2026-09-22-ecp5-model-replacement/` に保存する。

## 認定結果

`native-01` で実測モデルが `lut40196` を `R52C99/SLICEC.K0` から
`R55C99/SLICED.K0` へ移し、全体STAはsetup **0 ps** / hold **+90 ps**。
CPU **124.8 MHz**、メモリ62.4 MHz、setup/hold予約各200 psを維持した。
RTL相当の論理回路、プリミティブ設定、packing・クロック構成は元と一致する。

この実装のbitstreamをFPGAへ書き込み、**267/267試験**に合格。
固定仕事量の266試験は従来と同じ命令数、mcycle差はすべて±1以内。
コンソール出力は686バイト完全一致。最後のatomic OSは
mcycle=127145 / minstret=41092で合格し、基板には新しいCPUを残した。

採用プロファイルは [qualified-measured-placement.json](../qualified-measured-placement.json)。
bitstreamは `riscv/build/riscv-hardware-compact-atomic-measured-124p8-20260922.bit`、
SHA-256 `c3aa587f90f5569c514d1b39d906eb9fce382ace345f35a16a2a866fc2f1190f`。
認定記録は実行ディレクトリの `native-01/qualification/qualification.json`、
入力・ソース・成果物の再検査は `validation.json`。

## 適用方法

Ricaで実測配置を使う入口は `riscv/tools/texo-measured pnr ...`。
通常のPnR引数に `--measured-placement-model` を付加して実行する。
モデルや実測入力が見つからない・ハッシュが違う場合は失敗し、旧式へ戻らない。
他のデバイス・パッケージ・速度グレードへの適用も拒否する。

```sh
cargo build --release \
  --manifest-path toolchain/worktrees/isa/texo-measured-timing/Cargo.toml \
  -p texo-cli --target-dir target/texo-measured-timing
python3 riscv/characterization/run_native_model.py \
  --output toolchain/measurements/2026-09-22-ecp5-model-replacement/native-next
python3 riscv/characterization/qualify_native_model.py \
  toolchain/measurements/2026-09-22-ecp5-model-replacement/native-next
```

最後のコマンドは、STAとJTCK検証・チェック付きbitgenを通した後にFPGAへ書き込み、
命令セット262件・OS 4件・console 1件を試験する。
失敗時には従来の認定済みCPUへ復帰する。

ツール変更は [texo-measured-placement.patch](texo-measured-placement.patch)。
元の `texo-atomic-placement-owners` ソースへの差分であり、元ツールは変更しない。
変更前後のファイルハッシュは実行記録の `source-changes.json` にある。

## ネイティブ実行経路

- `MeasuredPlacementModel::load` が32個の測定入力のSHA-256とモデル形式・検証誤差を確認。
- 実測モードでは従来の `Ecp5PlacementDelayPredictor` を生成しない。
  旧距離式による仮のSTA・配置フィードバックも実行しない。
- 初期配置は指定した配置、または接続関係に基づく配置を使用する。
  まず実際に配線してSTAを行い、そのslackを変更候補の優先順位に使う。
  STA遅延を回帰モデルの教師値には使わない。
- 対象は測定と対応するK0 F→K0 A/B/C/D、単一論理sink、非零距離、
  主軸12タイル以内・副軸2タイル以内。範囲外は未評価のまま記録する。
- 同じ接続集合の変更前後を比較し、少なくとも半数が測定範囲内で、差が
  `max(100 ps, 2 × holdout誤差 × 変更前コスト)` より大きい候補を選ぶ。
  リング固有のゲート・観測回路のoffsetは各接続へ加算しない。
- 合法な候補を最大4件まで試行する。変更セルに接続した全netを配線し直し、
  全体STAでsetup/holdの両方が合格し、最悪slackが悪化せず、
  検査数・未検査endpointが維持された候補だけを採用する。
  失敗した候補は元の実装に影響しない。強制保存routeがある場合は移動を拒否する。
- checkpointの `target.measured_placement` にモデルSHA、適用範囲、
  全候補の採否と配線後WNS/WHSを保存する。

このコストはLUTと配線を合わせた平均値であり、netだけのSTA遅延には代入しない。
実測モデルのスコア差を、そのCPU経路で実測した高速化量とも呼ばない。

## STA表の置換範囲

配置用の推測式と、配線後STAのタイミング表は別のもの。
今回のネイティブモードは配置用推測式を使用せず、最終STAは既存の
Project Trellisタイミング表で検証する。

`collect_physical_routes.py` とネイティブの `measured_surface` で、
測定画像とCPUの624,552配線接続すべてをTexoのPIPへ照合した。
`audit_replacement_coverage.py` はRO本体の閉路だけを辿り、
観測用カウンタなどの配線を測定サンプルに混ぜない。
CPUが使う配線クラスは368種、RO本体で測定した閉路にあるのは180種。
さらに28種は測定画像全体の配線の和集合にも存在しない。
キャリー・広幅LUTのセル経路、および独立したclkQ/setup/holdも不足している。

従って `coverage-report.json` はSTA表の全面置換を不合格としている。
任意のCPU経路に対する実測STAへの全面移行は未完了であり、
現在のデータから独立した各arcの値を捏造しない。

## ソフトウェア検証

`cargo test --release -p texo-flow -p texo-cli` は139件合格。
新規テストは未測定接続の拒否、リングoffsetの二重計上防止、
hold悪化・制約消失候補の拒否、測定ファイル改変の拒否を確認する。
Clippyは既存コードの `unused_self` / `too_many_lines` / `missing_panics_doc` と、
BEL名 `.K0` をファイル拡張子と誤認するlintを明示的に許可した実行で合格。
無条件の `-D warnings` の失敗ログも保存する。

実機認定後の再検査は以下。FPGAの書換えは行わない。

```sh
python3 riscv/characterization/validate_native_model.py \
  toolchain/measurements/2026-09-22-ecp5-model-replacement
```
