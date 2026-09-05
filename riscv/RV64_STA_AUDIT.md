# RV64：STA通過イメージのJTAG不動作と検査範囲の監査

後続調査で、固定配置配線のままJTAG受信シフタのエッジ変更による復旧を確認し、
RTLを修正した。通常128-bit通信、8-byte WRITE64、実機100 ELFs／134実行が合格。
JTAG内部515経路の追加STAとbitgen前の拒否ゲートも実装した。
[原因・修正・確認範囲](RV64_JTAG_FIX.md)を参照。以下は修正前の監査記録。

2026-09-06。対象ツールは実際の生成に使用したTexo 0.1.6、revision
`7b372da4997c924b48cc09327ef006b7af753e65`。保存済みログとこのrevisionの実装を照合した。
この監査では再書込みや周波数変更を行っていない。

## 結論

**STAを通過してbitgenも成功したが、実機のJTAG SELECT応答がゼロになるイメージは存在する。**
一方、これだけで遅延データの精度不良やSTAの計算誤りを特定したとは言えない。
確定した問題は、必要な回路を検査しない状態でも `timing_closure` を掲げてbitgenへ進めること。

これまでの「STA合格」はCPU／メモリの検査済み範囲に限る。
現在採用したイメージには100 ELFs・127実行の実機機能検証があるが、
**全ドメインのSTA認定や、PLL jitterを含む独立した動作余裕の証明はない。**
前の結論でも、この限定を明記すべきだった。

## 観測した事実

| イメージ | CPU／memory | setup / hold WNS | 実機 |
| --- | --- | --- | --- |
| `20260906.bit`、SHA先頭 `f76f0907` | 124.8 / 62.4 MHz | +14 / +290 ps | JTAG SELECT応答がゼロ。再試行でも同じ。CPU命令試験の成立を確認できない |
| `20260906-ce.bit`、SHA先頭 `ed8b1237` | 同じ | +11 / +290 ps | 100/100 ELFs、反復込み127実行が合格 |

2件の全RTLハッシュは一致する。追加したCE fanout制約はcacheの `matched_entry_q` と
小型予測器の `decoded_target_q` / `decoded_fallthrough_q` の各8分割であり、
JTAGの制約やRTLを直接修正した実験ではない。配置配線と実装セル数は異なる。

失敗時にもJTAG IDCODE `0x81113043` は読め、同じWindows XVC接続で従来の静的版を
書き戻すと基本試験は合格した。ただし、SELECTの全ゼロは応答選択が更新されない場合にも
起こり得るため、これをPLL unlockやCPU本体の周波数限界の確定証拠として扱わない。

[失敗ログ](../toolchain/measurements/2026-09-06-rv64-compact-predictor/hardware/compact-initial.log)、
[再試行](../toolchain/measurements/2026-09-06-rv64-compact-predictor/hardware/compact-retry.log)、
[静的版への書戻し](../toolchain/measurements/2026-09-06-rv64-compact-predictor/hardware/static-recovery.log)、
[採用版の全実機集計](../toolchain/measurements/2026-09-06-rv64-compact-predictor/summary.json)。

## ソースで確認した検査の穴

1. **未検査でも合格する。** 失敗版のSTAは `met_timing=true` と同時に
   `all_modeled_endpoints_checked=false`。13,902端点を検査する一方、520端点を除外した。
   519件はJTAGの `unconstrained_clock`、1件はCDC初段の `no_synchronous_launch`。
   shift register、mailbox、request toggleが含まれる。CDC初段の通常setup検査の除外と、
   JTAG領域内の同期経路の未制約は、同じ理由で許容してよいものではない。
2. **bitgenは検査範囲をゲートしない。** `TimingReport::met_timing()` は検査済みsetup/holdの
   最悪余裕が非負かを見る。`all_modeled_endpoints_checked()` は別関数。
   bitgenの入力検査は `met_timing` を読むが、未検査端点の妥当性を判定しない。
3. **JTAGG自身のアークがモデルに入らない。** adapterはJTAGGを `ResourceKind::Logic` にし、
   `ecp5_timing_model()` はLUT4、FF、BRAM、clock buffer、carry以外をスキップする。
   JTAGG内部のlaunch/captureを追加せず、単にJTCKの周波数を設定しても全経路検査にはならない。
   520件は「モデルに存在しているが未検査」の数で、モデル化されていない全経路の数ではない。
4. **非同期resetのrecovery/removalを検査していない。** `add_setup_hold_timing()` は
   `LSR` を通常のsetup/holdから除外する。同期データの制約へ混ぜないこと自体は正しいが、
   この版のSTAには別のrecovery/removal検査がない。JTAG側には非同期resetを使うFFがある。
5. **追加のsetup uncertaintyは0。** 全13,902 setup検査で `uncertainty_ps=0`。
   flowはnextpnrとのQoR比較に合わせた方針を明記している。
   +11 / +14 psをPLL jitter込みの実機余裕として読んではならない。

参照した固定revisionのコード:

- [合格判定と検査範囲](https://github.com/fabrica-eda/texo/blob/7b372da4997c924b48cc09327ef006b7af753e65/crates/texo-timing/src/lib.rs#L441)
- [bitgenの通過条件](https://github.com/fabrica-eda/texo/blob/7b372da4997c924b48cc09327ef006b7af753e65/crates/texo-cli/src/bitstream.rs#L403)
- [モデル生成・LSR除外](https://github.com/fabrica-eda/texo/blob/7b372da4997c924b48cc09327ef006b7af753e65/crates/texo-flow/src/lib.rs#L3096)
- [クロック制約の方針](https://github.com/fabrica-eda/texo/blob/7b372da4997c924b48cc09327ef006b7af753e65/crates/texo-flow/src/lib.rs#L2986)
- [JTAGGの分類](https://github.com/fabrica-eda/texo/blob/7b372da4997c924b48cc09327ef006b7af753e65/crates/texo-struo/src/lib.rs#L763)

## モデルとSTAのどちらか

今回実行したSTAはTexo自身の計算器であり、nextpnrの実行結果ではない。
遅延モデル名は `nextpnr_ecp5_project_trellis_min_max_ps`。したがって、
nextpnrへ置き換えた結果が一致しても、共有する遅延データの精度を独立に証明したことにはならない。

- **確定:** 制約・モデルの検査範囲、および全体合格への昇格条件が不足している。
- **未確定:** 検査したパスの遅延数値が実機より楽観的か、STAの演算・エッジ対応に誤りがあるか。
- **未確定:** 故障がJTAGのsetup/hold、reset、CDC、あるいは配置配線・構成生成の機能不良のどれか。

論理マッピングの等価性検査は、生成した物理配線・設定bitの完全な機能検証を代替しない。
ボードのpost-map JTAG回帰も、既存の `NonScalarControl` によりignoreされている。
[既存回帰](verify/tests/hardware_jtag_write.rs)と
[WRITE64の別件](../toolchain/mre/rv64-jtag-write64/README.md)を参照。
別件での低TCK試験結果を、今回の全ゼロ不良の試験結果に転用しない。

## 切り分けの順序

1. JTAG内部クロック・UPDATEのedge関係とJTAGGのアークをモデル化し、領域内setup/hold、
   reset recovery/removal、CDCをそれぞれ検査する。未制約を残してよい端点を理由付きで限定する。
2. 失敗版のPLL lock／reset／mailbox受理を分けて実機観測する。TCK低下だけではhold違反を
   除外できないので、最大遅延と最小遅延の両方を調べる。
3. 失敗した配置配線を固定し、物理設定の接続性・機能を検証する。
4. 検査対象と配線を揃えて独立したSTA／遅延根拠と照合し、遅延モデルと計算器の責任を分ける。

上記は初回監査時の切り分け計画。実施結果と残るモデル不足は冒頭の修正報告に記録した。
