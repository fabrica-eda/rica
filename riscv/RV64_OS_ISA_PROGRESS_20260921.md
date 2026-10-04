# OS動作再確認・syscall拡張とA拡張の演算部

以下は同日の最初の到達点の記録。続くCPU統合・Rust原子操作の状態は
[A拡張プラットフォーム](RV64_ATOMIC_PLATFORM.md)を参照。

継続作業では、LR/SCと全9種のAMO（W/D）をCore/LSUへ接続し、Rustの
原子操作を使うkernel variantまで実装した。A診断72実行、既存回帰476実行、
kernel 15実行がモデル上で合格し、通常処理のサイクル数を維持している。
既存の認定済み124.8 MHzイメージでは、OSのタスク切替・fault復帰・追加syscall・
consoleを再度実機確認した。続く2026-09-22（JST）には、**A対応イメージも
同じ124.8 MHzで配置配線・実機認定を完了**した。A診断24件、既存238件、
OS 5種類の計267実機実行が合格し、既存238件の命令数は旧実機と一致、
サイクル差は全件±1以内だった。setup/hold各200 ps予約後で0/+90 ps、
consoleはモデルと同じ686 byte。認定bitstream・ハッシュ・実行手順は
[A拡張プラットフォーム](RV64_ATOMIC_PLATFORM.md#hardware-qualification-2026-09-22-jst)を参照。
以降の「Core/LSU未接続」は最初の到達点の状態を示す。

2026-09-21。既存OSと拡張OSを **CPU 124.8 MHz／メモリ62.4 MHz／JTCK 6 MHz**
で確認した。今回の実機は従来の認定済み `HardwareCompactConsoleTestTop` を再使用する。
bitstream SHA-256 は
`692bfec9d3228f5452d038de078f25ef7dd590e7bba90bdfd15f13892160bea8`。
周波数はPLL設定値。今回独立した周波数測定や新しい配置配線は行っていない。
従来のsetup +2 ps／hold +90 ps（各200 ps予約）の認定を引き継ぐ。

## 確認結果

WindowsのD2XX XVCブリッジを従来の `127.0.0.1:3721` で起動し、FTDI interface Aへ接続。
保存された旧ELF 3種類と新ビルド4種類、syscall版の追加2回、計 **9実機実行が合格**した。
既存3種類は新旧ELFの全PT_LOADについて、アドレス・サイズ・属性・ロード内容が一致する。
ELFファイル全体のSHAはデバッグ情報変更で異なるため、同一ELFとは扱わない。

| kernel | 実機mcycle | 実機minstret | 同時edgeモデルmcycle |
| --- | ---: | ---: | ---: |
| tasks（旧・新） | 86,653 / 86,653 | 30,725 | 86,653 |
| access-faults（旧・新） | 92,235 / 92,234 | 31,949 | 92,235 |
| syscalls（新、3回） | 125,413 / 125,412 / 125,412 | 41,206 | 125,413 |

4種類×3 clock評価（mem→cpu、cpu→mem、同時edge）の **12モデル実行が合格**。
consoleは旧版・新版とも実機で `ping`→`pong` とMEI受信、8例外からの復帰、2タスク完走を確認。
新版の686 byteのログは3 clock評価のモデルと一致した。
consoleの実機サイクル数にはUSB/host応答待ちが含まれるため、固定処理性能の比較には使わない。
syscallsは追加の仕事を行うため、従来tasksとの実行時間の差をCPU性能低下とは扱わない。

## OSの追加機能

`kernel-syscalls.elf` は既存の例外復帰・タイマー切り替えに加えて、
タスクID取得、時刻取得、共有64-bitカウンタのfetch-add、未定義syscallのENOSYS応答を持つ。
[ABI](kernel/README.md#optional-syscall-abi)はa7で操作、a0で引数と結果を渡す。
照会・更新は呼出元へ復帰し、yield回数とタイマー期限を変更しない。

2タスクが各16回更新し、タスクごとの結果ビットマップで0〜31を重複・欠落なく取得したことを検査。
両タスクへの実際のタイマー割り込み、32回のyield、レジスタとstack上の値の保持も引き続き必須。
共有更新の不可分性は単一hart・非ネストtrapで保証する。ISAのAMO実装やU-mode隔離ではない。

Nix環境でCargo wrapperが `+1.98.1` を解釈せずビルド不能だったため、kernelビルドと
モデルrunnerビルドは `rustup run 1.98.1 cargo` と同じsysrootのrustcを明示するよう修正。
runnerの出力先も固定し、環境の `CARGO_TARGET_DIR` と実行ファイルの位置が食い違うことを防いだ。

## ISAの進捗と次の接続条件

`src/AtomicAlu.veryl` にAMO.W/DのADD、SWAP、XOR、OR、AND、MIN、MAX、MINU、MAXUを追加。
入力保持と結果生成を別edgeに分け、responseは消費まで保持する。
W形式はrs2上位を無視し、rdへ返す旧値を符号拡張する。書込み値は下位32 bitを出力する。
[公式Zaamo仕様](https://raw.githubusercontent.com/riscv/riscv-isa-manual/main/src/unpriv/zaamo.adoc)
に対応する演算結果を独立したRust oracleと比較した。

nativeとECP5 mappedのそれぞれで **8,450 transaction** を検証した。
全32 funct5×2幅×100境界入力、2,048ランダム入力、演算中と未消費応答中のresetを含む。
不正funct5、wordの符号、overflow、入力変更、backpressureも確認。Veryl checkも合格。

**この演算部はCore/LSU未接続で、CPUでA命令が実行できるようになったわけではない。**
`misa.A` と既定bitstreamは変更していない。追加演算部の125 MHz向けmappingは
配置配線後のタイミング認定ではなく、将来の統合後に124.8 MHzを維持できるかは未検証。
次の工程は以下の順で進める。

1. decode・LSUのAMO read/modify/write直列化。alignment、RAM属性、fault、副作用とIRQ境界を確認。
2. LR/SCの物理reservationと無効化、aq/rl・FENCE順序、context switchを独立試験する。
3. OSから実際のA命令を使用するvariantとACT試験を追加。
4. 124.8 MHz、setup/hold各200 ps予約、JTCK照合、checked bitgen、exact hash実機を通して認定。

## 再現と記録

```sh
python3 riscv/kernel/check.py
cargo test --release --locked --offline -p verify-riscv --test atomic_alu
(cd riscv && veryl check)
# 先に従来の認定imageをprogramした同一実機で実行する。
target/release/hardware-riscv-tests --no-program --xvc 127.0.0.1:3721 \
  riscv/build/rust-kernel/kernel-syscalls.elf
```

生ログ、入力hash、ELF hash、モデル結果、実機カウンタはローカルの
`toolchain/measurements/2026-09-21-rv64-os-syscalls/verification.json` と同ディレクトリに保存。
測定出力は既存方針どおりGit管理対象外。継続用のモデル記録は
`riscv/build/rust-kernel/check.json` にもある。
