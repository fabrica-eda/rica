# FF・BRAM・DSP・負荷配線の基板内測定

`phase_sweep.py` は同一の配置配線に対してPLLの捕捉位相だけを変え、
新しいデータが捕捉できる境界を測る。LUTリングの平均伝搬モデルとは別の測定である。
温度の実測は行わず、捕捉予算に20%と200 psの仮定マージンを別フィールドで加える。
これは温度範囲の保証値ではない。CPUの124.8 MHzと既存STAの予約値は変更しない。

## 回路と採用条件

- 公称12 MHzの基板クロックからPLLのVCOを624 MHz、送出・捕捉クロックを各15.6 MHzに設定する。
  位相の最小設定単位はVCOの1/8周期、約200.32 ps。基板クロックの絶対精度は外部校正していない。
- 送出FFは毎サイクル反転する。捕捉クロックの立上がりでデータを保存し、
  送出クロックの立下がりで期待値と比較する。評価回路には半周期の余裕を取る。
- PLLロック後、基準クロック2^20周期待つ。その後2^18回比較し、
  立上がり・立下がり各2^17回について、新値との不一致と旧値との一致を別々に凍結してJTAGで読む。
  多ビットの混合値は両方に一致しないため、旧値の安定捕捉と区別できる。
  同じフレームを2回読み、ヘッダー・ロック状態・試行数・全カウントを確認する。
- 監視付き回路では別途、基準クロック2^19周期中の両PLL出力をカウントする。
  基準の1.3倍（境界の同期による±2カウント）でなければデータを採用しない。
- 配置配線後にFF、各LUTの真理値と直列接続、指定数の負荷、BRAMとDSPのモードを検査する。
  位相ごとの構成データはCLKOS2_CPHASE/FPHASE以外が同一でなければ拒否する。
- 早い捕捉で全回旧値、遅い捕捉で全回正解、という両端を観測できた経路だけを境界モデルに入れる。
  再びエラーが出る非単調な経路は通常の境界値として採用しない。

32経路の内訳：

| 対象 | 条件 |
| --- | --- |
| FF→LUT→FF | LUT 0/2/6/12段 × 4配置 |
| 負荷配線 | 捕捉FF 1/4/16/64個 × 2配置（送出FFの帰還・比較用負荷は別途存在） |
| BRAM出力 | 18-bit NOREGの同期読出し→捕捉FF × 2配置 |
| BRAM入力 | 送出FF→ADA4→BRAMの捕捉クロックによる同期読出し × 2配置 |
| DSP | レジスタを使わないMULT18X18D、4組の交互入力パターン |

DSPのうち1組は積が同じになるオペランドの交換で、グリッチ観測用の対照。
伝搬境界の学習には使わない。BRAMは交互の0/全1ワードを読む。
全アドレス・書込み・CE/リセット・全DSP入力パターンの最悪遅延試験ではない。

## 値の意味と適用範囲

得られる境界は、送出素子のclock-to-Q、配線、組合せ回路、捕捉素子のsetup/hold、
クロックの相対的なずれが合成された経路の特性である。これらを独立のSTAアークへ分解しない。
早い側と遅い側の境界を両方保存するが、早い側をFF単体のhold値とは呼ばない。

`analyze_phase.py` は生のカウントと構成データのハッシュを検査し、
`aperture-model.json` に極性別の全観測、境界区間、マージンを付けた捕捉予算を保存する。
遅延の教師値として既存STA表やシミュレータの値は使わない。

電源は現状のまま。基板回路図のコア電源はLT3508と抵抗で設定されている。
XVC経由での電圧変更・ADC測定系はなく、電圧は実測・掃引済みとは扱わない。

## 実測結果とモデルの使用

採用データは `toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01/`。
57位相を順方向・逆方向に測った **114イメージ、956,301,312回の捕捉**。
対照の同積DSPを除く31経路のうち、15経路で予算を作り、16経路を検証専用にした。
11種すべてで、検証経路の早い側・遅い側の境界が予算内に入った。
PLL出力の実測カウントは両側とも681,574または681,575で、期待値681,574.4の許容範囲内。

以下は今回の配置・パターンで観測した位相であり、素子単体の遅延値ではない。
マージン付き予算は学習経路のみから計算し、検証値で拡大していない。

| 経路 | 全回新値一致となった境界の観測範囲 ns | マージン付き捕捉予算 ns |
| --- | ---: | ---: |
| FF→FF | 0.601–1.002 | 1.402 |
| FF→LUT 2段→FF | 2.204–3.005 | 3.806 |
| FF→LUT 6段→FF | 4.006–4.407 | 5.248 |
| FF→LUT 12段→FF | 7.212–7.412 | 9.094 |
| 捕捉FF 1負荷 | 1.002–1.202 | 1.402 |
| 捕捉FF 4負荷 | 0.601–0.801 | 0.921 |
| 捕捉FF 16負荷 | 0.801–1.002 | 1.402 |
| 捕捉FF 64負荷 | 1.002–1.202 | 1.642 |
| BRAM出力→FF | 3.205 | 4.046 |
| FF→BRAM入力 | 0.601–0.801 | 1.162 |
| FF→DSP→FF | 2.804–3.405 | 3.565 |

負荷の違う回路は配置も違うため、この表から負荷数だけの単調な遅延式は作らない。
同積のDSP対照では、入力交換途中の位相で不一致を観測した。
この回路はグリッチの観測記録として残し、通常の伝搬境界モデルから除外した。

[生データを結び付けた境界モデル](../../toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01/aperture-model.json)、
[検証済みの経路予算モデル](../../toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01/budget-model.json)、
[再現検査](../../toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01/validation.json)。
大きなbitstream・構成データ・生フレームはGit管理外に保存している。

`apply_phase_budget.py` がこのモデルを読み、旧値を捕捉する−0.200 nsと
新値を捕捉する＋9.215 nsを選択した。それぞれ独立に3回FPGAへ書き込み、
**追加50,331,648回の捕捉**で32経路すべての期待する旧値／新値を確認した。
[適用・実機再検証記録](../../toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01/confirmation/application.json)。
温度自体を変えて検証したわけではない。

最後に認定済みatomic120へ復帰し、atomic OSも合格した。
CPU 124.8 MHzのまま、mcycle=127145、minstret=41092。
この追加モデルは測定回路の捕捉予算選定に使用したもので、CPUのPnR/STA表の置換ではない。
LUT配置順位付けへの実測モデル適用は、README記載のリング測定による既存の試行を参照。

モデルを参照・再検証する例：

```sh
python3 riscv/characterization/phase_budget.py \
  --model toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01/budget-model.json \
  --family dsp_comb
python3 riscv/characterization/validate_phase.py \
  --output toolchain/measurements/2026-09-22-ecp5-phase-aperture/aperture-01
```

## 実行

```sh
python3 riscv/characterization/phase_sweep.py build --output NEW_DIRECTORY
python3 riscv/characterization/phase_sweep.py sweep --output NEW_DIRECTORY \
  --steps=-8:49:1 --repeats 2
python3 riscv/characterization/analyze_phase.py --output NEW_DIRECTORY
python3 riscv/characterization/test_phase.py
```

測定はFPGAのSRAMを書き換える。終了時・Python例外時には認定済みatomic120へ戻し、
atomic OSを実行して `restore.log` に結果を残す。プロセス強制終了や電源断の場合は
README記載の復帰コマンドを手動で実行する。

初期試行 `pilot-01`〜`pilot-06` は測定基盤の診断用で、モデルの採用データではない。
省略したPLL出力選択がnextpnrの構成データでREFCLKになっていたため、
分周出力の指定を明示し、構成データの検査と実機クロックカウンタを追加した。
元の構成データと生カウントは削除せず残している。
`fixed-01` はPLL修正の確認、`monitored-01` は周波数監視と新値捕捉の予備測定。
旧値を直接比較する `aperture-01` を早い側・遅い側両方の境界測定に使う。

一次資料：
[Lattice sysCLOCKガイド](https://www.latticesemi.com/view_document?document_id=50465)、
[評価基板ガイド・電源回路図](https://www.latticesemi.com/view_document?document_id=52479)。
HDLの旧値参照はIEEE 1800-2023 §4.9.4を確認した。
