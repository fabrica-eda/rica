# RV64 store応答短縮と命令補充の改善

2026-09-06。**124.8 MHzを維持し、実機のload/store処理率を10.4%向上させた。**
対象は LFE5UM5G-85F、CABGA381、speed 8。CPU 124.8 MHz／メモリ62.4 MHz、JTCK 6 MHz。
単発行RV64、64行BTB＋2-bit方向予測＋8段RAS、4 KiB L1Iを維持する。

## 採用した実装

`HardwareCompactStore4KTestTop`（`FAST_LSU=2`）を採用した。
storeは有効な全byte bankを書き終えたメモリedgeで応答し、従来の追加1メモリcycleを省く。
未完了storeを先に退役させないため、FENCE／FENCE.I／tohostの順序は保つ。

命令cacheは、現在行がhitしている状態でPC+8の補充が戻ったら、そのedgeでPC+16の要求を提案する。
以前はPCが進み、同期tag bankが次の読出しを返すまで待っていた。
storeが早く終わると、この位置関係の変化で5 ELFsが逆に遅くなっていた。
先読みはPC+16までで止め、PC停止中に要求を出し続けない。

Texoには最遅経路のセルを局所的に移動する処理を追加した。
未変更配線を保持し、移動した接続と共有分岐を再配線する。全体のSTAで目的関数が
厳密に改善する候補だけを確定する。29回の採用移動でsetup −706→+6 psとなった。
広い範囲の配置変更時に配線を凍結する先行案は効果がなく、最終変更から外した。

`FAST_LSU=0`は旧応答latencyの比較用、`1`はstore転送bufferの試作比較用。
今回の実機認定は**2のみ**。転送bufferの採用を意味しない。

## 実機の性能

Windows D2XX／8-byte WRITE64で**100 distinct ELFs・118実行が全合格**。
基本8、RV64I 51、Zca 32、診断9本を各3回。各実行で11カウンタも検査した。
以下は同じELFの3回の中央値。IPC=`minstret/mcycle`、MIPS=`IPC×124.8`。
比較元は直前のnative-JTCK版（bit SHA `c748eec0…`）。

| 診断 | 実機cycles（旧→新） | 新IPC | 新MIPS | 処理率向上 |
| --- | ---: | ---: | ---: | ---: |
| alu_chain | 5,995 → 5,993 | 0.770232 | 96.1249 | 0.033% |
| branch_pattern | 8,644 → 8,643 | 0.326854 | 40.7914 | 0.012% |
| call_return | 4,947 → 4,947 | 0.312513 | 39.0016 | 0.000% |
| load_store | 5,448 → 4,934 | 0.261451 | 32.6291 | 10.418% |
| capacity_3k | 8,861 → 8,859 | 0.696918 | 86.9754 | 0.023% |
| capacity_6k | 20,482 → 20,480 | 0.301221 | 37.5923 | 0.010% |
| fence_i | 1,825 → 1,802 | 0.039956 | 4.9865 | 1.276% |
| btb_conflict | 3,461 → 3,458 | 0.299595 | 37.3895 | 0.087% |
| call_nested | 19,329 → 18,305 | 0.280524 | 35.0095 | 5.594% |

`load_store`はIPC **0.236784→0.261451**、**29.5507→32.6291 MIPS**。
`call_nested`は**5.594%向上**。他の診断にcyclesの退行はなく、1～3 cycle程度の差は
大きな性能向上として扱わない。これは固定した診断の結果であり、汎用アプリケーション全体の倍率ではない。
モデル上の実行区間4,659→4,145 cyclesと、実機CSRの5,448→4,934 cyclesは別の測定区間。
両者を混ぜてIPCを算出しない。

## タイミングと同一bitstreamの確認

- 最終checkpoint: `build/lsu-refill/final-store.texo.json`。
- setup **+6 ps**／hold **+290 ps**。追加setup uncertaintyは0 ps。
- 14,437 setup／hold端点を検査。JTCKの515 setup／515 holdは同じcheckpointを使った
  追加STAとnominal slack・launch/capture edgeが全一致。
- 例外は既存のJTAGG外部3境界とCDC初段1境界だけ。JTAGG内部のlaunch/capture、
  reset recovery/removal、CDC自体の数値保証を追加した結果ではない。
- 通常bitgenで生成した既定イメージ:
  `build/riscv-hardware-compact-store-4k-124p8-20260906.bit`。
- bit SHA-256: `e325997fb3b7c4d283d4e2d2ad192b2c0a550639ce8acd4cfc953f210136b942`。
- 最終RTLと整理後のTexoからの再生成も、実機118実行に使ったbitstreamと完全一致した。

余裕はまだ小さい。クロックの引下げやSTA例外の追加で合格させていない。

## 検証と再現

固定100 ELFs × 両clock実行順のモデル200実行が合格し、基準版よりcyclesが増える例は0。
PC停止中の先読み上限・アドレス周回を回帰に加え、旧cacheに戻すと失敗することも確認した。
連続debug byteの回帰は、現行のパラメータ付きMemoryへ別の未使用パラメータを挿入するよう更新した。
旧テストの文字置換はパラメータなしの宣言専用だったため、追加宣言を作れなくなっていた。
LSU=0/2をnative／WASM、O0/O1/O2で検査する24構成が合格した。
Rica全体は**159 PASS、既存ignore 1**。変更targetのClippy、Rust fmtと変更Verylのfmtも合格。
採用後の通常runnerでもモデル200実行が同じ計測値で合格し、既定bitstreamの実機8実行を追加確認した（合計126実行）。

```sh
cargo run --release --locked --offline -p verify-riscv --bin verify-riscv -- \
  --hardware-memory-rtl --compact-predictor --icache-kib 4 --fast-lsu 2
cargo run --release --locked --offline -p verify-riscv --bin hardware-riscv-tests -- \
  --xvc 127.0.0.1:3721
```

PNRは[完全な引数とソースSHA](../toolchain/measurements/2026-09-06-rv64-lsu-refill/final-store-pnr.json)を使い、
Veryl入力を現在のcheckoutの`riscv/`へ置き換える。既存出力は上書きしない。
Texo修正は[PR #65](https://github.com/fabrica-eda/texo/pull/65)（`c5976713`）として提出し、CIのRust検査も合格した。
基準コミットは`41a1dc30`、追加変更は
[最終patch](../toolchain/measurements/2026-09-06-rv64-lsu-refill/texo-final-eco.patch)。
通常STA→`tools/check_integrated_jtck_sta.py`→通常bitgen→同一SHAの実機検査を維持する。
[測定・トレース・実行ログ](../toolchain/measurements/2026-09-06-rv64-lsu-refill/README.md)と
[実機の全sample](../toolchain/measurements/2026-09-06-rv64-lsu-refill/hardware-comparison.json)を参照。

## 次の改良

1. 同じ124.8 MHzでsetup余裕を拡げる。局所配置を合格済みcheckpointから再開し、
   ソース変更後も未変更部分の配線を引き継げるようにする。
2. load応答の待ち短縮を比較する。転送bufferは追加FFと比較経路の負担も測り、
   固定クロックのSTA・モデル・同一SHAの実機を通すまで既定にしない。
3. 方向誤りとL1I容量の改善は、残る実機cyclesと想定するプログラムを基準に選ぶ。
