# S-mode（Supervisor）

2026-10-05。[OS/ISA計画](RV64_OS_ISA_PLAN.md) §3の2段目として、S-modeとトラップ委譲を
`ENABLE_S`（`ENABLE_U` が前提）で実装した。作業用U/PMPプロファイルの
`HardwareCompactUserTestTop` で有効。アドレス変換は**Bareのみ**で、Sv39は次の段階。
**112/56 MHzでタイミングを閉じ、実機で270 ELFが全件合格**した。

## 実装範囲

| 項目 | 内容 |
| --- | --- |
| 特権 | M/S/U。`MRET` はMPP=S/Uへ、`SRET` はSPPへ戻る。S/Uへの復帰でMPRVを0にする |
| 委譲 | `medeleg`（例外0〜9、12、13、15。11=M-mode ECALLは常に0）と `mideleg`（SSI/STI/SEI） |
| トラップ | M未満で委譲された原因は `stvec` へ。`sepc`/`scause`/`stval`、SPP/SPIE/SIEを更新。Mからは委譲しない |
| S CSR | `sstatus`（SIE/SPIE/SPP/MXR/UXL）、`sie`/`sip`（委譲済みビットのみ見える）、`stvec`（directのみ）、`sscratch`、`sepc`、`scause`、`stval`、`scounteren`、`senvcfg`（0固定）、`satp` |
| `mstatus` | SIE/SPIE/SPP/MXR/TVM/TW/TSR、SXL=UXL=64。MPPは00/01/11（10は00になる） |
| 割り込み | SEI/STI/SSIのpendingはソフトウェア所有（Mは `mip` で3つ、Sは `sip` で委譲済みSSIPのみ書ける）。宛先がMのものを先に、各宛先内はMEI>MSI>MTI>SEI>SSI>STI |
| カウンタ | Sは `mcounteren`、Uは `mcounteren & scounteren` を満たすとき `cycle`/`instret` を読める |
| 命令 | `SRET`（UとTSR=1のSで不正）、`SFENCE.VMA`（UとTVM=1のSで不正。それ以外はNOP）、`WFI`（TW=1のSとUで不正） |
| `satp` | 0固定、書込み無視（未対応MODEの書込みは無効、という仕様上の扱い）。TVM=1のSでは不正 |
| `misa` | S（bit 18）を立てる |

STIPなどS割り込みの外部ソースはない。M側ソフトウェアがタイマ割り込みを受けて `mip.STIP`
を立てる、SBI型の運用を想定する。

### PMPとS-mode

PMPはSとUを区別しない。このため、PMPだけではS-kernelをU-taskから保護できない。
U-taskの隔離を伴うS-kernelはSv39（PTEのUビット）と一緒に導入する。
現在のU/PMPカーネル（M-kernel＋U-task）はそのまま動く。

## 構造

- CSR索引を5 bitから6 bitに広げ、32〜44をSバンクにした。読み出しは既存と同じく
  索引ビットによる二分木の多段選択で、既存バンクの経路に優先順位チェーンを足さない。
- 委譲判定はトラップ原因が決まった時点で登録する。例外はEXで（データアクセス違反は
  load/storeの別も含めて）、atomicユニットの違反は候補2つ（misaligned/access）をEXで
  登録してCMで原因の最下位ビットで選び、割り込みは受理時に登録する。`CsrFile` は
  `exception_delegated`/`interrupt_delegated`（M中は0）を出し、コアが提示する
  `trap_supervisor` に従って `trap_vector` とトラップ状態の更新先を選ぶ。M側の状態
  （`mepc` など）は委譲しないトラップでだけ更新する。原因からの組合せ判定は、
  トラップ値64 bitのイネーブルに深い経路を作ってタイミングを悪化させた。
- パイプラインでは `SRET` を `MRET` と同じxRETとして扱い（障壁・フラッシュ・予測抑止を共有）、
  SRETフラグで復帰PC（`sepc`）と状態遷移を選ぶ。
- 特権に関する不正判定はRXで行う。M未満からのCSRアクセスはアドレスの特権フィールドで判定し、
  Sバンク以上への書込みは既存のCSR書込みと同様に後続命令をフラッシュする。

## 検証

| 検証 | 結果 |
| --- | --- |
| `verify/tests/supervisor_csr.rs`（native＋ECP5 mapped） | 委譲、SRET、状態の見え方、割り込みの宛先と優先順位、カウンタ、TVM/TW/TSR、MPRV |
| `verify/tests/csr_decoder.rs` | S CSR 13個の索引と6形式の合法性、SRET、SFENCE.VMA（rd≠0とfunct7違いは不正）。`ENABLE_S=0` では全て不正 |
| 既存CSR/PMPテスト | `ENABLE_S=0` で全て合格（CSR索引の拡幅後） |
| `tests/rv64-s/supervisor.S`（コア全体、14段） | 両クロック順と同時クロックモデルで合格。委譲されるload/store/atomicのアクセス違反を含む |
| 変異試験 | S関連のRTL変異19個のうち17個を `supervisor.elf` が検出。残る2個はatomicユニット経由の委譲で、Sへ委譲される場合はこの経路を通らない（M宛ての場合は検出） |
| 既存270 ELF | Sなしで全合格。Sありでは2つ（`system`、`machine_interrupts`）がM/Uプラットフォーム固有の `mstatus`/`mie` 値を厳密比較していたため、MISAのSビットで期待値を切り替えるよう修正 |
| U kernel（同時クロック、Sあり） | mcycle 231,189、minstret 66,473。112 MHz記録と同一 |

`supervisor.S` の各段は、期待するトラップ（原因、xEPC、継続先、継続特権）をMとSのハンドラが
照合する。EBREAKは委譲しないので、Mへ戻る手段として使う。ビルドは `kernel/build-user.py`。

## タイミングと実機

S有効時は、修正前のTexo（`main` の素のフロー）では112/56 MHzを閉じられなかった（WNS −444〜−669 ps）。
論理段数は増えていない（最大12段）。原因は初期配置で、計測タイミングライブラリ使用時は
配置用の遅延予測が無効のため、初期配置がタイミング重みなしになる。小さなRTL変更で初回配線後の
違反が301件から5,826件まで振れていた。Texo [PR #102](https://github.com/fabrica-eda/texo/pull/102)
で、初回配線後のSTAのcriticalityから大域配置を解き直す段を追加し、S有効で
**setup 0 ps／hold +1 ps**（194 psガード、既存4例外）で閉じた（1,121 s）。
同じ版で、Sを無効にした変種と旧HEADも閉じる。

| 項目 | 値 |
| --- | --- |
| bitstream | `riscv/build/riscv-hardware-compact-user-112-supervisor-20261005.bit` |
| SHA-256 | `09f213ee2cdf16db46383b82d6f7d38936cd7eed4bb77182e39c29cf2935d97a` |
| チェック付きbitgen | プロファイル・PLL照合、JTCK 515 setup／515 hold一致 |
| 実機 | 270 ELF全件合格（tapir、5分50秒）。`supervisor.elf` mcycle 5,430（同時クロックモデル5,431）、U kernel mcycle 231,191／minstret 66,473（モデル231,189／66,473） |

記録は `toolchain/measurements/2026-10-05-rv64-s-112/`（配置配線・bitgen・監査・ソース、
実機ログ、native検証、変異試験）。RicaのTexoピンはPR #102のマージ後に上げる。
