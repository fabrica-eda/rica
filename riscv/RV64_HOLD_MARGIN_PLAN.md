# RV64 DSP版のhold余裕拡大

2026-09-08。基準は実機認定済みの `HardwareCompactDspM4KTestTop`
（実装 `00aae16`、測定 `3a94495`）。CPU 124.8 MHz、メモリ62.4 MHz、
JTCK 6 MHz、追加setup余裕200 psを維持して、hold余裕を拡げる。

## 調査結果

最小hold余裕は4 ps。メモリの `instr_response_address_q[9]` から
命令キャッシュの `response_address_q[9]` への最小配線遅延は496 ps、
hold側のデータ到着3701 psに対して要求3697 ps。この端点のsetup余裕は
追加200 psを差し引いた後でも6526 psある。200 ps未満のhold端点は71個。
[経路と観測値](../toolchain/measurements/2026-09-08-rv64-hold-margin/baseline-short-paths.json)に保存した。

Texoの既存hold修復は負のslackだけを対象とし、4 psで合格すると終了する。
RTLを変更する前に、明示したhold余裕をSTA制約と修復処理へ渡し、短い経路の
配線を延ばす方法を試す。対象デバイスの遅延モデルは変えない。

## 実施計画と採用条件

1. Texoへ既定0 psのhold uncertaintyを追加する。capture側のhold要求に一度だけ
   加え、setup、周期、生成クロックの位相関係を維持する。端点ごとの予約値を保存する。
2. 同一クロック、逆エッジ、関連クロック、既定0 psの回帰を確認する。
3. 認定済み配置配線から再開し、まず200 psのhold余裕を予約する。setup余裕200 psも
   維持した完全STAと配線検査に合格した候補だけをbitstream化する。
4. JTCKの追加STAと構造検査を通し、実機133 ELFsと16診断の反復で機能・IPCを確認する。
   同一ELF、同一クロックの実測値で性能低下がないことを確認する。
5. 採用結果、未達候補、コマンド、入力と成果物のSHAを新しい測定ディレクトリへ保存し、
   TexoへPR、Ricaへ依存固定と測定結果をコミットする。

前回の封印済み測定とbitstreamは比較基準として保持する。JTAGG hard macroの
未characterize境界、reset/CDCの検査限界は既存の記録を引き継ぐ。

## 配線探索の不具合

hold余裕を200 psにした最初の候補 `hold-01` は、71端点の不足から2端点へ減ったが、
予約後のhold -33 psで未達だった。追加予約前のholdは+167 ps。setupは+24 psを維持した。
この候補をbitstream化していない。

原因はTexoの配線探索で、既存または先に配線したsetup枝の最大遅延を、hold枝の最小
遅延へ混ぜていたこと。共有区間の最小10 ps／最大200 psに最小10 psの枝を足す
小さな回帰では、実際の最小20 psを210 psとして扱い、100 ps要求を誤って満たした。
修正後は最小130 psの迂回配線を選ぶ。独立したSTAは混同しておらず、CPU候補でも
残った不足を正しく不合格にした。

探索するsinkがsetup／holdで切り替わるとき、同じscratch bufferの共有区間を対象の
cornerで再計算する。デバイス全体の追加配列を持たず、setupだけの通常配線には
再計算を加えない。hold後のsetup探索が最大遅延へ戻ることも回帰で検査する。

## 採用結果

**CPU 124.8 MHz／メモリ62.4 MHzで、hold余裕を+4 psから+200 psへ拡げた。**
setupの追加予約200 psを維持し、同じCPU RTLと同じELFで実機性能を確認した。

| STA（ps） | DSP導入時 | hold修復後 |
|---|---:|---:|
| 公称setup余裕 | +224 | +224 |
| setup予約 | 200 | 200 |
| 予約後setup slack | +24 | +24 |
| 公称hold余裕 | +4 | +200 |
| hold予約 | 0 | 200 |
| 予約後hold slack | +4 | 0 |

遅延モデルや周波数制約は変更していない。最終判定はsetup/hold各16,956端点を検査し、
従来の4境界例外だけを維持した。JTCKの515 setup/holdは、最終公開コミットから
構築した追加graphでも公称slackとedgeが一致した。

セル26,373個、DSP16個、net29,367本は同じ。71ネットの配線を変更し、PIPは
312,043から312,160へ117個増加。4組のLUT/FF専用接続を一般配線へ開放した。
セル位置と記録されたBEL情報は同じ。

最初の未達候補 `hold-01` に続き、修正済み `hold-02` を認定した。
公開Texo `6baf4626b17d6373c973755336460c9bc32ed458`
（[PR #69](https://github.com/fabrica-eda/texo/pull/69)、上流マージ済み）から再開した
`hold-03-final` もcheckpointとbitstreamがbyte単位で一致した。
Ricaの依存とJTCK probeをこのコミットへ固定した。

既定bitstream：`build/riscv-hardware-compact-dsp-m-4k-124p8-20260908-hold.bit`。

- bit SHA-256: `7617e167a88b41286b78e4b20ca610a4c87b5ea2f30767b13eca6ab539395545`
- checkpoint SHA-256: `a6ddbc374dcedd39591820cd2a68fa33d85a2594dd098687392dedc8489557ab`
- Texo binary SHA-256: `3fefc3e4572059795b585170182fcbfaaf81ce639cc43f485adc07368f05f1b6`
- architecture SHA-256: `b2fc7972942d1e32cb5613c302c8dbea92114959842305e6678bd691edcca1b5`

## 実機のIPCとクロック

Windows D2XX/XVC経由で同一bitをSRAMへ書き込み、133種類のELFと16診断×2反復、
計165回すべてが合格した。既定runnerの書込みと基本8本を2回確認し、総計181回が合格した。
CPUクロックは124.8 MHz、メモリ62.4 MHz、JTCK 6 MHz。以下は同じELFでの各3回の
中央値で、起動・検算を含むfreeze済みの `mcycle` / `minstret` を使う。

| 診断 | 旧cycles | 新cycles | retired | 新IPC | IPC×124.8 MHz（MIPS） |
|---|---:|---:|---:|---:|---:|
| alu_chain | 5,993 | 5,992 | 4,616 | 0.770360 | 96.140988 |
| branch_pattern | 8,643 | 8,642 | 2,825 | 0.326892 | 40.796112 |
| call_return | 4,946 | 4,946 | 1,546 | 0.312576 | 39.009462 |
| load_store | 4,423 | 4,422 | 1,290 | 0.291723 | 36.407056 |
| capacity_3k | 8,859 | 8,859 | 6,174 | 0.696918 | 86.975415 |
| capacity_6k | 20,480 | 20,481 | 6,169 | 0.301206 | 37.590508 |
| fence_i | 1,802 | 1,803 | 72 | 0.039933 | 4.983694 |
| btb_conflict | 3,459 | 3,458 | 1,036 | 0.299595 | 37.389474 |
| call_nested | 17,281 | 17,280 | 5,135 | 0.297164 | 37.086111 |
| array_copy | 22,475 | 22,474 | 7,464 | 0.332117 | 41.448216 |
| array_sum | 14,760 | 14,760 | 5,152 | 0.349051 | 43.561626 |
| pointer_chase | 12,658 | 12,659 | 4,113 | 0.324907 | 40.548416 |
| m_divide | 24,486 | 24,486 | 2,580 | 0.105366 | 13.149718 |
| m_multiply | 10,147 | 10,147 | 2,580 | 0.254262 | 31.731940 |
| soft_divide | 221,464 | 221,465 | 92,760 | 0.418847 | 52.272133 |
| soft_multiply | 116,591 | 116,590 | 43,915 | 0.376662 | 47.007393 |

乗算は10,147 cycles／IPC 0.254262のまま。16診断すべての中央値の差は±1 cycle以内で、
以前からの起動位相差の範囲に収まった。今回の改善は、実測性能を維持したhold余裕の確保。
CPUモデルのcycleをこのIPCへ転用していない。

Texoの444試験（既存ignore 5件）、Clippy、GitHub CIは合格。
Rica workspaceの171試験（既存ignore 1件）も合格した。CPU RTLとStruo/Celoxの版は
変えていないため、両clock順の133 ELFモデル試験は前回のDSP認定結果を引き継ぐ。
今回の実機検査と単体・mapped MDU試験は新しい依存で実施した。

生ログ、全ELF SHA、STA比較、再現コマンド、成果物、未達候補の記録は
[測定ディレクトリ](../toolchain/measurements/2026-09-08-rv64-hold-margin/README.md)に保存する。

## 次に行う改善

優先順位はMDUから実行段への引き渡しとメモリ待ちの削減。
乗算の外部加算段を縮める候補は、今回のsetup/hold各200 ps予約と124.8 MHzを
維持した状態で比較する。除算の早期終了や次のISA拡張も、この余裕指定と実機ゲートを使う。
