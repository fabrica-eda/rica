# ECP5 DSP推論とRV64M乗算短縮

2026-09-07。基準は `2ac59cc`／測定 `7aeade0`、実機認定済み124.8 MHzのM版。
DSPを使うためにStruo／Texo側も実装する、というユーザーの指示に基づく。
実装コミットは `00aae16`。

## 実装と採用の順序

1. Verylの乗算をStruoのword-level IRへ保持し、18×18以内のunsigned部分積を
   ECP5 `MULT18X18D`へmappingする。幅・符号・truncationは元のRTLの意味を維持する。
   formal／primitive simulation／JSON出力も同じ演算にする。
2. TexoへDSPのBEL種別、pin接続、配置、組合せ遅延、bitstream設定を追加する。
   最初は内部registerとcascadeを使わない18×18乗算に限定し、外側のFFでpipelineを組む。
   targetの既存characterizationを使い、未知のDSP設定やtiming不足は拒否する。
3. 小さな独立fixtureでRTLとmapped動作、通常STA／bitgenを確認する。
4. RV64M乗算を16-bit部分積へ分割し、DSPで短縮する。除算とstart／done／cancel契約は
   基準と揃え、MULH系とMULW、依存、flush、precise exceptionを回帰検査する。
5. CPU 124.8 MHz／memory 62.4 MHz、200 ps追加setup余裕を維持し、同一bit SHAの
   実機で133 ELFと診断の再測定を行う。現在のradix-4版と同じ仕事の処理時間で比較し、
   合格して速い候補を既定にする。失敗候補は採用しない。

調査時のStruo main `836236d`には乗算演算とDSP cellがなく、TexoもDSPのresource／STA／
bitgen経路がないことを確認した。独立checkout `toolchain/worktrees/dsp/struo` と
`toolchain/worktrees/dsp/texo`で対応する。既存の認定済みcheckpointと証跡は変更しない。

一次資料：

- [Lattice ECP5/5G sysDSP User Guide](https://www.latticesemi.com/view_document?document_id=50469)
- [nextpnr ECP5 primitive support](https://github.com/YosysHQ/nextpnr/blob/main/ecp5/docs/primitives.md)
- [nextpnr ECP5 configuration writer](https://github.com/YosysHQ/nextpnr/blob/main/ecp5/bitstream.cc)

新しいコマンド・観測値・失敗結果は
`toolchain/measurements/2026-09-07-rv64-dsp/`に記録する。作業中。

## 実装済みの構成

64×64のunsigned積を16個の16×16部分積に分け、128-bitの結果まで7段の
外部FFで合成する。各段の加算器を最大65-bitに抑え、DSP内部registerは使わない。
符号処理とMULWの切り詰めは既存MDUの前後処理を維持した。
除算器、cancel、結果保持、commit時の制御は従来の契約に従う。
DSP有効時には旧radix-4乗算の未使用ループを合成から除く。

Struoは `4e9de737`（[PR #88](https://github.com/fabrica-eda/struo/pull/88)）へ更新。
Celoxは `7ff0dd2`（[PR #762](https://github.com/celox-sim/celox/pull/762)、上流マージ済み）
へ統一し、0.3.1 adapterと試験的backportを削除する。必要なRustは1.98.1。
このCelox修正はstore分割の最適化を維持し、同じFF状態に対する先行readを
追い越してstoreしないよう配置位置を制限する。独立した状態の早期storeは維持する。
191本のStruo workspace試験、native／Cranelift／WASMの177入力の積と7段遅延を確認済み。

## STAとbitgenの修正

初期候補01/02は200 ps追加setup余裕込みのSTAに不合格だった。
候補07はplacement weight exponent 4でsetup +24 ps、hold +4 psに到達した。
JTCK 6 MHzを含む16,956 setup/hold検査と、従来と同じ4件の明示的境界例外を使用。

bitgenはDSPのC入力288本のうち144本の定数固定情報が欠けて停止した。
固定配線を1段だけ探索していたため、z=0/4のDSPへ至る3段のaliasを扱えなかった。
固定配線を終端まで探索し、循環を打ち切り、programmable経路や曖昧な定数元を
採用しないようTexoを修正した。候補09では不足が0本となりbitgenも成功した。
候補07から09で配線とSTA結果は一致し、独立JTCK graphの515 setup/hold結果も一致した。

候補09 bit SHA: `7297078f67fa530468cf57e13cce3bdcb600dbf20611788b4facf05abe021733`。
CPUモデルは新しい依存でも133 ELF × 両clock順の266回を通過した。
実機の133 ELFと診断16本×2回、計165回を同一bitで通過した。既定runnerへ切り替えた後の基本8本も通り、
合計173回。
Texoは `acdfe43`（[PR #68](https://github.com/fabrica-eda/texo/pull/68)）へ固定し、
439試験（既存ignore 5件）とCIのClippyも通過した。

## 実機性能と採用

同じ133 ELFのSHA、CPU 124.8 MHz／memory 62.4 MHzで比較した。
以下は各3回の中央値。ハードウェアのfreeze済みmcycle/minstretを使用し、起動と
検算も含む。同じELFの比較であるため、IPC×clockの改善が処理時間の改善に対応する。

| 256組の乗算ベンチ | radix-4版 | DSP版 |
|---|---:|---:|
| mcycle | 16,294 | 10,147 |
| minstret | 2,580 | 2,580 |
| IPC | 0.158340 | 0.254262 |
| IPC×124.8 MHz | 19.7609 MIPS | 31.7319 MIPS |
| 実行時間 | 130.561 μs | 81.306 μs |

乗算は **1.6058倍**、時間は **37.73%短縮**。
除算は24,486 cyclesで同じ。ALU、load/store、array sum/copy、pointer chase、
call_nestedも同じ中央値で、その他の主要ベンチは起動位相に伴う±1 cycleの範囲だった。
DSP版を実機runnerの既定にする。基準のradix-4版は比較用topとbitを残す。

CPUモデルは乗算9,630/9,629 cyclesで、実機10,147 cyclesと一致しない。
両モデルの停止点には従来から数cycle差があり、今回の差はそれだけでは説明できない。
MDU単体のRTLとマッピング後モデルは、radix-4/DSP双方で全13命令・境界入力の
結果と完了cycleが936演算で一致した。CPUモデルのpre-edge測定でもMDU待ちは
256命令×11 cycles。メモリ待ちはモデル1,285／実機1,793で、差508 cyclesが
全体差517 cyclesの大部分を占める。残り9 cyclesには停止点の違いが含まれる。
同時CPU/memory edgeを2通りに直列評価する現在のモデルは機能検証用であり、
同時edgeを一括評価するcycle精度は今後の課題とする。モデルのcycleを実機IPCへ転用しない。

## 次の改良の優先順位

1. **hold余裕を増やす**。現在+4 psで合格しているが、配置変更時の余地が小さい。
   124.8 MHzと200 ps追加setupを維持したまま、短いデータ経路の配置・配線を改善する。
2. **乗算後の依存待ちとメモリ待ちを減らす**。DSP数よりMDUの引き渡しと外部加算段を
   詰める方が、単発のRV64乗算には効く。段削減でclockを落とさないことを条件にする。
3. **除算の早期終了**。ゼロ、±1、被除数が小さい場合などを検出して反復数を減らし、
   通常経路は短く保つ。実測で除算が多いworkloadを選んで比較する。
4. **その後のISA拡張**。Mの次は用途を決めてZba/Zbb等の整数処理、またはAのatomicを
   検討する。Aには予約・例外・メモリ順序の設計が必要で、decoder追加だけでは済まない。
   F/Dは浮動小数点の用途と面積・検証費用を確認してから判断する。
