# 分岐予測器の追加診断

`cargo run --release --bin rv64-riscv-tests -- --predictor-tests --compact-predictor --max-cycles 100000`
でビルドし、単一クロックのコアモデルで結果の正しさを確認する。
性能比較には生成した ELF を次のボードメモリモデルへ渡す。

```sh
target/release/verify-riscv --hardware-memory-rtl --compact-predictor --icache-kib 4 riscv/build/rv64-predictor/btb_conflict.elf riscv/build/rv64-predictor/call_nested.elf
```

- `call_nested`: 同じ外側関数を2か所から呼び、さらに内側関数を呼ぶ。復帰先が交互に変わるため、最後の行き先だけを覚える BTB と RAS の差を測れる。圧縮命令の PC+2 復帰先も使う。
- `btb_conflict`: 64バイト離れたジャンプと条件分岐を繰り返す。半語 PC を索引にする32エントリ表では衝突し、64エントリ表では分離する。

これらは特定の機構を検証するための診断であり、既存4ベンチマークの幾何平均には混ぜない。初期化・結果判定を含む全体のサイクル数を記録し、モデル値と実機カウンタ値も分ける。
