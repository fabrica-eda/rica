# JTCK標準STAとRV64改善の実施記録

2026-09-06。対象はECP5 LFE5UM5G-85F EVN、CABGA381、speed grade 8。
CPU **124.8 MHz**／メモリ **62.4 MHz**を維持する。
JTCKの通常STAへの統合を採用した。タイミング余裕とLSUの改善案も実装・比較したが、
高速化候補は配線後のタイミングが未達のため採用しない。今回、実機のIPC向上は達成していない。

## 採用した変更と実機結果

Texo [PR #62](https://github.com/fabrica-eda/texo/pull/62) はマージ済み。
`jtagg.JTCK` をセル・出力ピンで指定する内部クロック制約を追加し、6 MHzのJTCKを
CPU／メモリと同じSTA処理で検査する。通常のbitgenが検査範囲とsetup／holdの合格を要求する。

- 最終checkpoint: `build/native-jtck/final-default.texo.json`
  （SHA-256 `9e95357a54516791961801d421ff502cef6787f9255e7d44dbc5f4c4c2408674`）。
- 既定bitstream: `build/riscv-hardware-compact-4k-124p8-20260906-native-jtck.bit`
  （SHA-256 `c748eec0c93bef04bb935c60ca282e116fc4f983a9d1cb010ebd69d432ba1424`）。
- setup **+1 ps**／hold **+290 ps**、14,406 setup／hold端点を検査。
  setupの100 ps未満は13端点。追加のsetup余裕指定は0 ps。
- JTCKの515 setup／515 hold端点は、同じ配線checkpointから別に構築した追加STAと
  nominal slack・launch/capture edgeが一致する。両者は同じTexo遅延モデルとSTAエンジンを使う。
- 未検査はJTAGG出力の3境界とCPUへ渡るCDC初段の1境界のみ。
  [例外ファイル](timing-coverage-exceptions.json)で端点と理由を固定した。
  JTAGG内部のlaunch/capture特性、reset recovery/removal、CDCそのものの安全性を
  数値的に保証した結果ではない。
- Windows D2XX／6 MHz TCK／8-byte WRITE64で**100 distinct ELFs／118実行**が合格。
  内訳は基本8、ACT4 RV64I 51、Zca 32、診断9本を各3回。
  最新RTLから再生成したbitstreamがこの実機合格済みイメージと完全一致することを確認した。
  既定パスを切り替えたrunnerでも追加の基本8本が合格した。

診断3回の中央値。IPCは`minstret/mcycle`、MIPSは`IPC × 124.8 MHz`。

| 診断 | cycles | 退役命令 | IPC | MIPS |
| --- | ---: | ---: | ---: | ---: |
| alu_chain | 5,995 | 4,616 | 0.769975 | 96.0929 |
| branch_pattern | 8,644 | 2,825 | 0.326816 | 40.7867 |
| call_return | 4,947 | 1,546 | 0.312513 | 39.0016 |
| load_store | 5,448 | 1,290 | 0.236784 | 29.5507 |

旧JTAG修正版との0～1 cycle差は高速化と評価しない。旧bitstream `5ac5d9e0…` の
setup +9 ps／hold +290 ps、100 ELFs／134実行は[以前の修正記録](RV64_JTAG_FIX.md)に残す。

## 実装・比較した高速化案

比較用に`HardwareMemory.FAST_LSU`を実装した。0は元の応答、1はstore応答短縮と
直近1件のSD→load転送、2はstore応答短縮だけを有効にする。実験用topはそれぞれ
`HardwareCompactLsu4KTestTop`／`HardwareCompactStore4KTestTop`。
これらの実装とテストは[不採用の試作コード](../toolchain/measurements/2026-09-06-rv64-native-jtck/lsu-prototype/)
に保存した。通常の`src/`は全31ファイルを実機合格済みの基準SHAへ復元した。

storeはBRAMの有効byteを書き込んだedgeで応答する。未完了storeの退役を許すbufferは
導入していない。同一の18-bit物理開始アドレスに対する64-bit storeだけを転送用に保持し、
部分storeとdebug writeは無効化する。重なる別アドレスのloadは通常BRAMを読む。
単体モデルではstoreが3→2メモリedge、転送hitのloadも3→2 edgeになった。
これはCPUの総cyclesや実機MIPSの改善を示す測定ではない。

RASのwrite enable明示、投機pushの登録とbypass、checkpoint割当スロットのbusy登録も
比較した。機能比較は通ったが物理タイミングが悪化したため、predictor RTLはすべて
採用前の版へ戻した。4096回の連続stack操作を比較する回帰テストは残した。

代表的な結果を示す。setup欄は余裕指定を差し引いた後のslackで、全行とも同じPLL設定。
全試行のコマンド・ソースハッシュ・結果は[測定記録](../toolchain/measurements/2026-09-06-rv64-native-jtck/)に保存する。

| 候補 | 予約したsetup余裕 | setup ps | hold ps | 判定 |
| --- | ---: | ---: | ---: | --- |
| native JTCK既定版 | 0 | +1 | +290 | 採用・実機合格 |
| RAS entry CE、fanout 4 | 0 | −3,518 | −134 | 不採用・RTL復元 |
| store短縮＋転送 | 0 | −880 | +290 | 不採用 |
| store短縮＋転送、配置重み4 | 250 | −1,740 | +290 | 不採用 |
| store短縮のみ | 250 | −1,971 | +290 | 不採用 |
| 元の31 RTLファイルを完全復元 | 250 | −1,119 | +290 | 余裕未達 |
| 元RTL、合格版の配置を引継ぎ | 250 | −654 | +290 | 余裕未達 |
| 元RTL、合格版の配置を引継ぎ | 100 | −504 | +290 | 余裕未達 |

初期の`margin250`試行はLSU無効時に未使用のpayload FFが残っていたため、元RTLの
対照実験としては使わない。generateでFFを除き、さらに全31ファイルのSHAを復元した
`original-margin*`を別に測定した。負のsetup／holdを持つ候補は実機へ書き込んでいない。
通常bitgenが`timing_closure`不足で拒否し、出力を作らないことも確認した。

## ツール側の改善

- [Texo PR #63](https://github.com/fabrica-eda/texo/pull/63):
  `--setup-uncertainty-ps`。PLL周期・位相を変えず、すべての配置配線試行で同じsetup余裕を
  要求する。423 workspace tests合格、既存ignore 5、example 3合格、fmt／clippy合格。
  100／250 psは今回の比較条件であり、実測したPLL jitterやPVT値ではない。
- [Texo PR #64](https://github.com/fabrica-eda/texo/pull/64):
  既存APIの初期配置とLUT/FFペア指定をCLIへ公開。#63を基準にした独立差分。
  CLI 44 tests・fmt・clippy合格。両PRのCIも合格している。

配置だけを引き継いでも再配線でnominal setupが+1→−404 psへ悪化した。
変更後RTLへの部分引継ぎは、物理group／共有資源の条件に合わず検証で拒否された。
論理機能の一致を物理配置の合法性と同一視できない。配置入力は前の配線・STA合格証跡を
引き継ぐ機能ではなく、良い結果の再現や性能向上を保証しない。

## CPUモデルの追加検査

LSU試作のCPUモデルは、無効設定であっても連続debug writeのデータを1 byteずらして読み、
起動しなかった。旧`HardwareMemory`に**未使用のパラメータ宣言を1つ追加するだけ**でも
nativeで再現する。WASMでも不一致を観測したが、frontend／scheduler／backendの責任範囲は
まだ特定していない。[再現コードと観測値](../toolchain/mre/celox-parameterized-memory/README.md)を残した。
この失敗からIPCを算出していない。依存ライブラリの試験的変更もすべて戻した。

通常RTLを元の31ファイルへ戻した後、連続debug byteの回帰テストと、固定100 ELFsを
CPU・メモリ同時edgeの両方の実行順で検査する**200回のCPUモデル実行**が合格した。
Rica全体は**154 tests合格・既存ignore 1**。変更した対象のclippyとfmtも合格した。
全targetのclippyには既存の警告が残っており、全target合格とは記録していない。
LSUの試作実装はタイミングとモデルの両方が通るまで通常ソースへ戻さない。

## 再現する際の入口

基準の全PNR引数は[baseline-pnr.json](../toolchain/measurements/2026-09-06-rv64-native-jtck/baseline-pnr.json)、
追加したTexo機能を含む最終ビルドは[final-default-pnr.json](../toolchain/measurements/2026-09-06-rv64-native-jtck/final-default-pnr.json)
に記録した。ソース、tool binary、checkpoint、bitstreamのSHAを混同しない。
初回のsource snapshotと通常RTLの31 SHAは一致する。最後の再生成はLSU=0の試作宣言を
含む別snapshotだが、生成されたbitstreamは初回と完全一致する。

新しい候補は既存の出力を上書きせず、記録されたCE fanout／PLL／JTCK制約を引き継ぐ。
PNRが合格した後に`tools/check_integrated_jtck_sta.py`で構造と追加STAを照合し、
通常の`texo bitgen`で生成する。CPUモデルと実機はその候補のSHAを固定して検査する。

## 次の判断

1. **124.8 MHzで実機合格した既定版を保持する。** JTCK制約・4境界の検査・通常bitgenを
   今後の候補にも必須にする。
2. **配置配線の改良を優先する。** 配置に加えて未変更netの配線を保持し、変更部分だけを
   検査・再配線するフローが必要。失敗した試行で合格済みの解を失わず、同じ作業量で
   slackと実行時間を比較できるようにする。現在のCLI引継ぎはその前段階まで。
3. **LSUはstore応答短縮のみから再評価する。** 転送bufferより変更が小さく、マップ上の
   増加は1 LUT。固定クロックのsetup／hold、モデル、同一SHAの実機100 ELFsをすべて
   通したうえで、`load_store`の総cyclesと他診断の退行を判定する。

現時点で100／250 psの余裕確保と実機IPC向上は未達。STA未達を許可したbitstreamや
クロックを下げた結果で、今回の高速化を達成したとは扱わない。
