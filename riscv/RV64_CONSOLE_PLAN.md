# USB/JTAGコンソールとRust kernel

2026-09-11。実装・実機認定済み。既定は `HardwareCompactConsoleTestTop` の
`riscv-hardware-compact-console-124p8-20260911.bit`。

ECP5-EVNのUART配線は標準状態ではR34/R35が未実装なので、既存USB/JTAGを
最初のホスト接続にする。根拠は[基板マニュアル6.2](https://www.latticesemi.com/view_document?document_id=52479)。
CPU稼働中のログ出力と受信を実装し、UARTへの置換は後工程とする。

## 実装する契約

- `ENABLE_CONSOLE=1` とmachine timerを有効にした構成で提供する。
- 0x0200_1000〜0x0200_103fを、既存の実行不可・自然整列MMIO領域内に割り当てる。
  CoreのRAM範囲・64-bitアドレス検査・通常load/storeの待ち段は維持する。
- 送受信各16 byteのFIFO。CPUとホストの同時pop/pushを扱い、満杯時にも
  同時popで空いた場所へpushできる。空への同時pushは次の読出しから見える。
- 受信データあり×IRQ_ENABLEでMEIPを駆動する。受信FIFOを空にするかmaskして解除する。
- CPU停止/resetでFIFO・割り込み設定・エラーをクリアする。tohostによるタイマー停止後も
  ホストは残った出力を読み出せる。
- JTAG ER1 opcode 5でhost exchangeを要求する。data低byteが入力文字、address bit0が
  TX pop、bit1がRX push。CPU→memoryの保持bundleとtoggleで一度だけ処理し、
  応答は既存SELECTのbank7へ固定する。後続要求は完了を待つ。

| MMIO offset | 動作 |
| --- | --- |
| 0x00 TX_DATA | low byteへのwriteで1文字送信。満杯なら破棄しTX overflowを立てる |
| 0x08 RX_DATA | low byteを含むreadで1文字受信。bit8がvalid。空なら0 |
| 0x10 STATUS | bit0 TX space、bit1 RX valid、bit2 TX pending、bit3 RX space |
| 0x18 IRQ_ENABLE | bit0 RX interrupt enable。その他は0 |
| 0x20 ID | 読出し専用の識別値0x434f4e01 |
| 0x28 COUNTS | bits4:0 TX count、bits12:8 RX count |
| 0x30 ERRORS | bit0 TX overflow、bit1 RX overflow。W1C、新しいエラーを優先 |
| 0x38 reserved | 0を返しwriteを無視 |

8-byte slot内の小さいアクセスはlittle-endian。low byteを含まないTX/IRQ/ERROR writeと
RX readは副作用を起こさない。全MMIO要求はmemoryのrequest tokenに従い一度だけ実行する。

JTAG EXCHANGE完了応答はSELECT 224〜255（bank7）で読む。
bits7:0=取得したTX byte、bit8=TX pop受理、bit9=RX push受理、bit10=処理前TXあり、
bit11=RX空きあり（同時CPU popを含む）、bits23:16=完了したcommand sequence、bit24=busy。
その他は0。次のEXCHANGEはbusy解除とsequence一致を待つ。SELECTは副作用を持たない。
実行中の連続交換ではTAP resetを挟まない。CPU停止・再起動は既存CONTROLで行う。

## 機能検証済みの範囲

- 独立FIFO oracleをnativeとECP5 mappedモデルで確認。LSUのheld token、RAM alias、
  timer共有、maskとresetを確認し、JTAG→CPUの実行中交換・完了後の出力回収も通った。
- Rust `kernel-console.elf` が3種類のclock評価で起動、8件の例外ログ、MEIでの
  `ping\n` 受信と応答、タイマーでの2タスク完走を確認。ログはホストでも照合する。
- 既存238 ELF × 2 clock順序 × console有効/無効で952 PASS。
  同一37 ELFの同時edgeモデルは、追加前と命令数・cycle数が全件一致。
- `python3 riscv/kernel/check.py` は3 variantの計9実行に対応し、全件合格。

計測・入力SHAは[2026-09-11の記録](../toolchain/measurements/2026-09-11-rv64-console/)。
trial 33でsetup **+2 ps／hold +90 ps**、各19,690検査に合格した。
条件は124.8/62.4 MHz、JTCK 6 MHz、setup/hold各200 ps予約のまま。
配置配線・STAに探索変更を含まないツールでcheckpointをバイト単位で再現し、
JTCKの各515検査も別構築グラフと一致した。元のchecked bitgenで生成した
同一bitstreamを実機に書き込み、console kernelは3回とも686 byteのログがモデルと一致した。
既存238 ELFと反復78回、console kernel 3回の計319実行が合格した。
既定化・ホスト再ビルド後の8実行も合格し、同一bitstreamで計327実行を確認した。
固定処理35本は同一ELF・同一命令数で、従来実機・同時edgeモデルとの差は最大1 cycle。
[認定記録](../toolchain/measurements/2026-09-11-rv64-console/qualification.json)・
[IPCと設定クロック](../toolchain/measurements/2026-09-11-rv64-console/PERFORMANCE.md)。

配置配線では、LSUアドレス加算器→RAMの経路と、変更時に遅くなる周辺経路をまとめて修復し、
全体のSTAで採否を決めるようにした。最終段階は局所配置6件・配線3件の改善で閉じた。
探索中の悪化した中間状態は実装に採用していない。

## 順序と合格条件

1. FIFOとレジスタの独立oracleをnative/mappedで検証。held token、同時full pop/push、
   wraparound、byte lane、W1C、reset、mask/pendingを含める。
2. CPU wrapperとJTAG exchangeを接続し、Rust kernelにconsole driver、起動・faultログと
   MEI受信処理を追加する。既存kernel variantはコンソールなしでも起動できるよう保つ。
3. 同一ELFの既存CPU回帰と新kernelを検証する。OSの期待文字列と受信数をホスト側でも照合する。
4. 124.8/62.4 MHz、setup/hold各200 ps予約、JTCK 6 MHzでSTAとchecked bitgenを通す。
   合格したSHAを実機で検証し、IPCとクロックを記録してから既定化する。

完了後はA拡張→U/PMP→S/Sv39へ進む。
