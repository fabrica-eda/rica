# FPGA実測による配置コストモデル（2026-09-22）

現在のRica実行経路は [実測STA表全体の置換](STA_REPLACEMENT.md)（2026-09-23採用）。
`riscv/tools/texo-measured` が全1,552項目の実測表と実測配置モデルを選択する。
CPU 124.8 MHz／メモリ62.4 MHz、温度余裕20%、setup/hold各194 psで
全体STA +2/+1 psと267実機試験に合格した。
以下は配置コストモデルを先行導入した段階の記録であり、現在のSTA置換範囲は上記を参照。

FF経路・BRAM入出力・DSP・1〜64負荷の配線についても、
[PLL位相掃引による実機測定](PHASE_APERTURE.md)を追加した。
114イメージ・約9.56億回の捕捉から31経路の両側の境界を採用し、
15経路で作ったマージン付き予算を別の16経路で検証した。
以下のLUTリングモデルとは別に、経路全体の捕捉予算として扱う。

接続済みのLFE5UM5G-85F-EVNと基板の公称12 MHzクロックで計測し、
LUT→LUTの配置候補を順位付けするモデルを作った。外部測定器は使用していない。
**96経路、ウォームアップ後408測定**を採用し、72経路で学習、24経路で検証した。
学習に使わなかった経路の最大誤差は **4.25%**、同じ経路の反復測定の最大幅は
中央値に対して **0.0251%**。係数の教師値にシミュレータやSTAの遅延を使用していない。

[実測モデル](../../toolchain/measurements/2026-09-22-ecp5-measured-delay/measured-model.json)・
[検証記録とハッシュ](../../toolchain/measurements/2026-09-22-ecp5-measured-delay/validation.json)。
生のXVCフレーム、カウント、配置配線済み回路、bitstream、ツールのハッシュは
同じディレクトリの `registered-{A,B,C,D}-{x,y}/` に保存している。
これらの大きな生成物はGit管理外。

## 測定方法

- 入力ピンA/B/C/D、横/縦、33/65/97段、タイル間隔1/4/12を変えた反転LUTのリングを使う。
  別位置の49段リングを検証用とし、係数の学習には使わない。
- 発振するリングは同時に1つだけ。イネーブルは基準クロックのFFから直接駆動する。
  測定窓は `2^20` 基準クロック、約87.381 ms。
- リングの立上がりを4-bitプリスケーラで16分周して32-bitカウンタで数える。
  停止から1024基準クロック以上待って差分を保存する。
- JTAGは保存した結果を読み出すだけで、USBの待ち時間を測定窓に使わない。
  同じsequenceのフレームを2回読み、magic・経路番号・カウントが一致した値を採用する。
- 起動後の最初の一巡（sequence ≤ 12）は、値に関係なくウォームアップとして除外する。
  各経路はその後少なくとも4回計測し、範囲が1%を超えればモデル生成を拒否する。
- すべてのLUTの位置、INIT、選択した入力ピンへの閉ループ接続を配置配線後に検査する。

カウント `C` から求めるリング一周の平均伝搬遅延は
`D_ps = 10^12 × 2^20 / (12,000,000 × 16 × C × 2)`。
最後の2は、反転リングの一周期に二周の伝搬が必要なため。
立上がりと立下がりを平均した、LUTと配線を合わせた値である。

最初の組合せゲート版では、起動直後の1点に約3倍のカウントが出た。
その回路・データは `horizontal/`, `vertical/`, `{B,C,D}-{x,y}/` に残し、
最終モデルには使わない。組合せデコードの一時的な変化がゲートに入る可能性を除くため、
ゲートを登録した全8イメージを作り直して再測定した。原因を電気的に特定したとは扱わない。
登録版は、除外前の最初の一巡を含めても最大の範囲が約0.0293%で、同様の異常は観測されなかった。

## モデルの範囲

入力ピンごとに非負最小二乗で次の回帰式を当てはめる。

`D = a × LUT段数 + bx × Σmin(dx,5) + cx × Σmax(dx−5,0)
   + by × Σmin(dy,5) + cy × Σmax(dy−5,0) + offset`

距離の区切り5は回帰の基底であり、物理的な配線クラスを意味しない。
`a` はLUTと局所配線を合わせた切片で、LUT単独の遅延としては使わない。
`offset` にはゲートと観測分岐など、リング一個あたりの差が含まれる。
配置比較では同じ接続集合を比較するため、固定項は差分から消える。

| 入力ピン | 検証6経路の最大誤差 | 平均絶対誤差 |
| --- | ---: | ---: |
| A | 3.64% | 1.84% |
| B | 2.39% | 1.38% |
| C | 4.25% | 2.53% |
| D | 3.21% | 2.49% |

これは1枚の基板、実験時の条件での平均伝搬コスト。基準クロックの絶対精度、
電圧・温度は独立測定していない。FFのclock-to-Q・setup/hold、BRAM、DSP、
多負荷の配線、全PVTの最大最小遅延は、このモデルから生成しない。
したがってSTA用タイミング表全体の置換や、未測定条件での周波数保証ではない。

## PnRへの適用結果

[apply_model.py](apply_model.py)で既存CPUの配置候補を選び、Texoへ渡した。
対象は測定したK0 LUTのF→A/B/C/D、単一論理sink、近い距離の接続に限定する。
候補間で同じ測定対象の接続を比較し、対象外の接続は推測値で補完しない。
少なくとも半数の接続に測定範囲の裏付けがあり、予測差が検証誤差の2倍を超える候補を選ぶ。

最初の試行は `lut40196` を `R52C99/SLICEC.K0` から `R55C99/SLICED.K0` へ移動した。
接続5本のうち3本をモデルで評価し、残る2本を含む全体の配線・STAで合否を判定した。
予測したコスト差は約171.7 psだが、実機でこのCPU経路の改善量を測った値ではない。

- `--no-timing-optimization` により、既存の配置用距離予測器を無効にして実行した。
- RTL、CPU/メモリ/JTCKのクロック、200 psのsetup/hold予約は変更していない。
- 新しい配置は全体STAで **setup 0 ps / hold +90 ps**。
  独立したJTCK検査も各515件で一致した。
- WNSは元の配置と同じ。個々の配線には短くなったものと長くなったものがあり、
  この一試行で全体性能の改善を実証したとは扱わない。
- [適用入力とコマンド](../../toolchain/measurements/2026-09-22-ecp5-measured-delay/placement-01/application.json)・
  [STA比較](../../toolchain/measurements/2026-09-22-ecp5-measured-delay/placement-01/timing-comparison.json)。

この経路は実測モデルによる配置候補生成＋通常の配線検証であり、Texo全体の既定モデルを
一括変更するものではない。最終STAは独立した既存のタイミング表・検査を使い、
実測の平均値で予約や合否判定を緩めない。

試行CPUのbitstreamは書き込んでいない。計測後は認定済み
`riscv-hardware-compact-atomic-124p8-20260922.bit`（SHA-256
`3128297a36c001211e2c02fca5a354afd297c22962af068ebb6aedc7cddbf1b8`）へ戻した。
atomic OSは再実行に合格し、mcycle=127144/minstret=41092（同時edgeモデルとの差−1/0）。

## 再確認

リポジトリのルートから、保存した測定を検証する。

```sh
python3 riscv/characterization/test_measurement.py
python3 riscv/characterization/validate.py
```

旧Python版の実験を別の出力ディレクトリで再現する場合（通常は上記ネイティブ版を使う）：

```sh
python3 riscv/characterization/apply_model.py \
  --model toolchain/measurements/2026-09-22-ecp5-measured-delay/measured-model.json \
  --output toolchain/measurements/2026-09-22-ecp5-measured-delay/placement-next \
  --run
```

`generate.py`、`build.py`、`readout.py`、`sweep.py` が実機測定側。
`sweep.py` はFPGAを書き換え、既存の実験ディレクトリがあれば開始を拒否する。
読み出しだけなら `readout.py --seconds 30 --bitstream IMAGE --output RAW.json` を使う。
測定後の復帰は明示的に行う：

```sh
target/release/hardware-riscv-tests --xvc 127.0.0.1:3721 \
  --bitstream riscv/build/riscv-hardware-compact-atomic-124p8-20260922.bit \
  riscv/build/rust-kernel/kernel-atomics.elf
```

測定回路のビルドはYosys 0.68、nextpnr 0.11.1、ecppackを使用。
`build.py` に実際に使ったNix storeのパスを残し、各build.jsonに実行コマンドとハッシュを記録した。
この環境のnextpnr 0.6は発振回路を含む配置でクラッシュしたため採用していない。
発振回路だけは意図的な組合せループとして扱い、CPU認定の例外には流用しない。

HDLの読み出しレジスタはnonblocking assignmentの旧値評価を使う
（ローカルIEEE 1800-2023 §4.9.4 “Nonblocking assignment”を確認）。
回路プリミティブは[nextpnr ECP5 primitive support](https://github.com/YosysHQ/nextpnr/blob/main/ecp5/docs/primitives.md)、
構成ビットの生成は[Project Trellis](https://github.com/YosysHQ/prjtrellis)を利用した。
