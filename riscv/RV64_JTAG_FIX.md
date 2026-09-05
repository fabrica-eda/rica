# STA通過イメージのJTAG不動作：原因の切り分けと修正

2026-09-06。LFE5UM5G-85F / CABGA381 / speed 8。

**不動作箇所をJTAGGから128-bit受信シフタへの取り込みに絞り、RTLを修正した。**
CPU／メモリは **124.8／62.4 MHz** を維持。修正版は実機 **100/100 ELFs**、
反復・互換性検査を含む **134実行**が合格した。以前破損した8-byte WRITE64も、
全100 ELFsと、旧不具合の即値・位置検査 **48/48** が合格した。
ランナーの既定を修正版イメージと8-byte書込みへ更新し、既定設定での再書込み・8命令試験も合格した。

## 配置配線を固定した原因の切り分け

元の失敗イメージは `20260906.bit`、SHA-256先頭 `f76f0907`。
CPU／メモリのSTAはsetup +14 ps、hold +290 psだが、ER1 SELECT応答は全ゼロだった。
以下の比較では、このイメージの構成ファイルを基準に指定箇所だけを変更した。
CPU／メモリの配置、配線、LUT内容、PLL設定は変えていない。

| 変更・観測 | 実機結果 |
| --- | --- |
| 元の失敗版、TCK 6 MHz / 1 MHz / 100 kHz | すべてER1応答ゼロ。IDCODE正常、ER2のPLL lockは1 |
| JTDI一本の経路にモデル上+458 / +1,337 psの最小遅延を追加 | どちらも応答ゼロ |
| JTCKの制御4 FFだけを立ち下がりへ変更 | 128 / 129-bit scanとも応答ゼロ |
| 受信シフタ128 FFだけを立ち下がりへ変更 | 末尾に診断用1 bitを追加した129-bit scanで正常応答 |
| JTCKの132 FFを立ち下がりへ変更 | 同じ129-bit条件で正常応答 |
| 元の失敗版へ戻して同じフレーム境界調整 | 復旧しない |
| 再びエッジ変更版へ戻す | 正常応答を再現 |

構成差分、使用中の配線との衝突検査、各イメージのSHA、書込みログ、全selectorとsequenceの
読戻しを[実験記録](../toolchain/measurements/2026-09-06-rv64-jtag-zero/)に保存した。
この診断用ECOはSTA認定イメージとして扱わない。

これにより、CPUの再配置やクロック低下に頼らず、**JTAG受信シフタのエッジ関係を変えると
不良が消える**ことを確認した。JTDI一本の遅延だけでは説明できず、
JTDIとCE／SHIFTを同じ立ち上がりで取り込む設計が配線依存の競合を起こしていたと判断する。
JTAGG出力を立ち下がりで受け、制御を再登録する実装は
[Hazard3作者のECP5 JTAG実装](https://github.com/Wren6991/Hazard3/blob/stable/hdl/debug/dtm/hazard3_ecp5_jtag_dtm.v)
にも先例がある。ただし、根拠の中心は上記の自機での比較実験である。

**JTAGG内部のどのアークが何psのhold違反だったかは未計測。**
配線の推定遅延差を、そのまま実測のhold違反量とは呼ばない。

## RTLの修正

[JtagDebugTransport.veryl](src/JtagDebugTransport.veryl) を変更した。

1. JTAG側の261 FFをJTCKの立ち下がりで動かし、JTAGGの立ち上がり側の変化から離す。
2. `CE1 && SHIFT` を1段登録し、JTDIの1 TCK遅延に合わせる。
   最終bitを取り込む期間をRTL内で確保し、ホストの通常128-bitフレームを維持する。
3. JUPDATEをデータとして登録し、同じJTCKドメインでmailboxを確定する。
   JUPDATEを独立クロックとして使うFFと専用クロック配線を除いた。
4. UPDATEを一度だけ受理してER1選択を解除する。ER2／別DRによる古いコマンドの再送を防ぐ。
   CPU側のtoggle同期、mailboxの保持、PLL再lock時の整列は維持する。

再合成した31個のVerylソースについて、前の認定版との差はこの1ファイルだけ。
CPUコア、分岐予測器、cache、memory、PLL・ピン制約は同一である。

## STAが通ってしまった理由と再発防止

実行したSTAはTexo `7b372da4997c924b48cc09327ef006b7af753e65`。
遅延データはnextpnr／Project Trellis系だが、**今回の失敗経路は検査対象外だった**。
従来のレポートは519件のJTAG未制約端点と1件のCDC端点を残したまま `met_timing=true`。
JTAGGのlaunch/captureアーク自体も存在しない。
したがって、この事例からnextpnrの遅延数値の誤りやSTA演算の誤りを結論できない。
[固定revisionのソース監査](RV64_STA_AUDIT.md)を参照。

追加した `jtag-sta` はcheckpointの接続・配線遅延と同じspeed 8_5Gのセルモデルを読み、
**元と同じTexo STA計算器**でJTCKを6 MHzに制約する。再配線はしない。
さらに全261 FFの立ち下がり設定、UPDATE独立クロックの不在、端点数を検査する。
未検査を許すのは名前と理由を限定した3入力端点だけで、増加すれば失敗させる。

| 修正版の検査範囲 | setup WNS | hold WNS | 検査数 |
| --- | ---: | ---: | ---: |
| CPU／メモリ：通常の配線後STA | +9 ps | +290 ps | 各13,902 |
| JTCK領域のFF間：追加STA、6 MHz | +162,187 ps | +290 ps | 各515 |

追加STAの3端点は `shift_high[63].DI`、`shift_enable_q.DI`、`update_q.DI`。
これらのJTAGG側launch、JTAGGの他の入力境界、非同期resetのrecovery/removal、
CPUとのCDCは全域のタイミング認定に含めない。物理試験とRTLのエッジ／保持設計を併用する。
CPU側の追加uncertaintyは依然0であり、+9 psをjitter込みの動作余裕とは扱わない。

[bitgen_jtag_checked.py](tools/bitgen_jtag_checked.py) でこの検査をbitgenの前に実行する。
**元の失敗checkpointはexit 1で停止し、bitstreamを作らない。**
修正版は通過し、既に実機認定したものと同じSHAのbitstreamを生成した。
これはRica側の追加ゲートであり、Texo汎用CLIのモデル不足を解消したという主張ではない。

## 検証結果と性能

- Rust回帰152件合格、既存のfull-board post-map試験1件は従来のadapter制約でignore。
- JTAG単体のRTL／ECP5プリミティブ回帰は各196 ER1フレーム、全128位置のwalking bit、
  ER2／別DR 390フレーム、UPDATE保持、CPU再lockを検査。261 FFのエッジ保持も検査する。
- 実機100 ELFsは基本8、性能・cache・predictor診断9、ACT4 RV64I 51、Zca 32。
  全実行で11個の64-bitカウンタも検査した。
- 旧WRITE64再現試験は6即値×8配置位置がすべて一致。1-byte転送の基本8命令も合格。
- 既定ランナーで修正版を再書込みし、8-byte転送で基本8命令が合格。

3回の実機測定の中央値。起動・scrubから終端store応答までを含み、ELFのJTAGロードは除く。
クロック値はPLL設定値であり、外部計測器による周波数実測ではない。

| 診断 | cycles | retired | IPC | IPC × 124.8 MHz |
| --- | ---: | ---: | ---: | ---: |
| ALU | 5,994 | 4,616 | 0.770103 | 96.109 MIPS |
| branch | 8,645 | 2,825 | 0.326778 | 40.782 MIPS |
| call/return | 4,946 | 1,546 | 0.312576 | 39.009 MIPS |
| load/store | 5,449 | 1,290 | 0.236741 | 29.545 MIPS |

前の認定版の中央値との差は0～1 cycleで、CPU性能を保っている。
8-byte転送への復帰はホストのロード時のフレーム数を減らす変更で、CPUの高速化とは別である。

## 採用イメージと再現

bitstream: `build/riscv-hardware-compact-4k-124p8-20260906-jtag-falling.bit`

SHA-256: `5ac5d9e05e0cd2b82aec0b51039fae028f0081c5aa0c738e1bc89bfa69e15b12`

checkpoint: `build/jtag-zero/jtag-falling.texo.json`

SHA-256: `73418831502025fb155a71760eab3365e7d5cddfa3e56e7661757e077c61921c`

合成・配置配線の全オプションとRTL SHAは
[pnr記録](../toolchain/measurements/2026-09-06-rv64-jtag-zero/jtag-falling-pnr.json)、
測定値・全試験一覧は[summary.json](../toolchain/measurements/2026-09-06-rv64-jtag-zero/summary.json)。
実験ECOと追加STAの再現は[MRE手順](../toolchain/mre/rv64-jtag-zero/README.md)を参照。

```sh
cargo test --locked --offline -p verify-riscv --test jtag_transport_edges
cargo run --locked --offline -p verify-riscv --bin verify-riscv -- --jtag-only
cargo run --locked --offline --release -p verify-riscv --bin hardware-riscv-tests -- --xvc 127.0.0.1:3721
```

次の配置・機能追加でも、固定PLLのSTA、JTAG追加ゲート、同じSHAの実機回帰を採用条件とする。
JTAGGの公式launch/captureモデルとreset／CDC解析をTexo側へ追加する作業は別途残る。
