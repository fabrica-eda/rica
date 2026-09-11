# 定数のloop index加算がcarry回路として残る

Struo `4e9de737d1ad0b1e619d8887505a6ba0ad13c74b`、Ricaの固定依存で確認。
3-bit pointerから8個のslotをdecodeする、機能が同じ2種類のVerylを比較する。

```sh
cargo +1.98.1 run --offline --locked --release -p rica-mre \
  --bin struo-constant-loop
```

| 書き方 | 合成IRのarithmetic cells | mapped ECP5 cells | mappedモデル |
| --- | ---: | ---: | --- |
| procedural `for` 内の `(i + 1) as 3` | 8 | 144 | 全8入力PASS |
| generate `for` 内で `const NEXT_SP` を確定 | 0 | 8 | 全8入力PASS |

`Procedural.veryl` の加算は両入力とも定数だが、Struoのword-level arithmeticに
保持される。`struo-ir::Netlist::add_arithmetic_inner` はこの固定版では定数演算を
畳まずにarithmetic出力を作る。論理値の誤りではなく、不要なcarry回路と配線が残る問題。

CPUの `CompactReturnStack` に同じ式がcommitted/speculativeの2組あり、generate内の
定数へ移すとarithmetic cellsが132→116になった。変更前の `zba-06` の最悪経路では、
pointerとslotのFFはFPGAのrow 15〜18付近、定数carry回路と比較LUTはrow 65付近にあり、
往復の2本の配線だけで3,369 + 3,216 psかかっていた。これはTexoの保存済み
net delayからの診断であり、独立STAの結果ではない。

Rica側では定数を明示して回避した。依存ライブラリの変更は行っていない。
Struo全体での定数arithmetic foldingは別途適用できる改善として残す。

結果とCPUの経路は
[測定ディレクトリ](../../measurements/2026-09-08-rv64-zba/)、
`constant-loop-mre.log` と `zba-06-critical.json` に保存する。
