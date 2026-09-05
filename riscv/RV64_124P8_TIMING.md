# RV64：124.8 MHz必須条件でのタイミング改善

対象は LFE5UM5G-85F / CABGA381 / speed 8。CPU 124.8 MHz、メモリ62.4 MHz、
位相0の既存PLL設定を維持する。

**最終4 KiB版はTexoの配置配線後STAで合格した。setup +0.064 ns、hold +0.290 ns。**
bitstreamを生成済み。診断4本では、キャッシュなしに対してサイクル数で幾何平均
**1.470倍**相当の性能改善を保った。これは実機の速度測定値ではない。

**同じbitstreamを実機で検証し、124.8 MHz設定で98/98 ELFsと各11カウンタ検査が合格した。**
Windows側のFT2232HをD2XXで制御し、XVC経由で書込み・実行した。
検証条件は**1-byte単位のJTAG書込み**。8-byte書込みではpayloadが破損する不具合が
残るため、ランナーは検証済みの1-byte書込みとこのbitstreamを既定にした。
ロード時間は増えるが、CPUのPLL・RTL・実行速度を変える回避策ではない。

## 採用条件と性能の扱い

124.8 MHzでsetup/holdを満たすことを採用の必須条件とする。未達の構成は、
命令の実行サイクルが少なくても採用しない。PLL周波数の引下げでは解決しない。

[以前のL1I比較](RV64_L1I_IMPLEMENTATION.md)の1.575倍は、RTLの実行サイクルと
各構成の配線後Fmax中央値から算出した参考値だった。124.8 MHzでの性能値や実機測定値ではない。
4 KiB版を未達のまま「採用」とした前版の判断を撤回し、今回のSTA合格後に開発基盤とした。

性能は、合格した実装の同じ124.8 MHzにおける実行時間で評価する。
計算は `実行サイクル数 / 124.8 MHz`。段追加による待ち時間、分岐回復、
キャッシュミス、初期化を含めて測り、周波数だけを性能と呼ばない。

## 改修内容

- 命令キャッシュのEBR読出しを2段にし、EBRの出力遅延と取得制御を分割。
  読出しアドレス・有効性も同じ段数で保持し、両段のread/write競合を無効化する。
- メモリ応答のアドレス・sequence照合結果とデータを一緒に登録してから補充。
  `FENCE.I`は保持中の応答も破棄する。
- ミス要求の検出を登録し、次段でアドレスとsequenceを更新する。
  現在行と次行のアドレスを別々に保持し、タグ照合を要求アドレスの保持回路から分離する。
- 現在行の照合をPCへ直接接続。次行との隣接性は読出し段で登録し、
  行を跨ぐ32-bit命令の有効判定から次行アドレスの加算を外す。
- 静的予測のJAL・条件分岐では、相対変位が次PCと一致するかを前段で判定。
  JALRは必ず分岐先へ回復するため、加算結果の全32-bit比較を不要にする。
  JALRが偶然fallthroughへ向かう場合にも回復は入るが、retireは一度だけ行う。
- 静的予測の条件分岐は、EXで登録した比較片からCMで方向を決定。
  既存のCMでの回復時点を維持する。動的予測器のEX方向判定は保持する。
- EX由来の回復では、その命令と一緒にCMへ渡った分岐先を再利用。
  JALR加算の後ろに古いCM命令との優先選択・保持回路を重ねない。
- 静的予測で不要な予測PCのパイプライン転送を定数化。
- 圧縮命令展開とCSR番号の符号化を、独立した一致条件の並列合成へ変更。
  圧縮命令は全65,536通りで以前の展開結果のダイジェストと一致することを検査する。
- RXの待機中オペランドはRF応答とWB値から保持する。CM値はライブのforwardingで
  利用でき、CM→WB後はWB側から保存できるため、保持回路にCMの演算結果選択を含めない。

通常の命令幅・XLEN、メモリ容量、ISAの実装範囲を維持する。
キャッシュ読出しと補充に加えた段によるサイクル増は、性能比較に含める。

## 最終実装のタイミングと資源

合成対象は `HardwareCached4KTestTop`。Texo 0.1.6、合成目標125 MHz、配置重み指数4。
既存CSRのCE分岐に加え、FD・EXオペランド・CM演算値・取得FIFOのCEを8 FFごとに分岐した。
PLL、LPF、デバイス、speed gradeを変更していない。

| 項目 | 最終Texo実装 |
| --- | ---: |
| CPU / メモリ | 124.8 / 62.4 MHz |
| setup WNS / TNS | +64 ps / 0 ps |
| hold WNS / TNS | +290 ps / 0 ps |
| setup / hold検査数 | 各12,250 |
| LUT4 / FF / EBR | 11,138 / 4,685 / 146 |
| 実装セル / 配線ネット / PIP | 17,240 / 18,277 / 224,062 |

12,770モデル化端点のうち520端点はJTAGの独立クロック・非同期入力のため対象外。
内訳はJTAGクロック未制約519件、JTAG request toggleの初段に同期launchがない1件。
CPUコア、メモリ、両者の同期・関連クロック境界の未検査端点はない。
モデルはTrellisのmin/max遅延で、追加clock uncertaintyは0 ps。
**64 psはこのSTAモデルでの余裕であり、実機で確認した余裕ではない。**

同じ論理変更を使ったnextpnr 0.6-3build5の別実装は、seeds 1/2/5で
117.343 / 112.714 / 114.207 MHzとなり未達だった。採用する配置配線結果は
上記のTexo checkpointであり、nextpnr側まで合格したとは扱わない。

## 同じ124.8 MHzでのサイクル比較

最初のcommitから終端commitまでを両端含みで数える。キャッシュなしの比較側も
最終Coreで実行し直した。2通りのCPU/メモリ同時エッジ順序で下表の実行区間は一致した。
時間欄はRTLのサイクル数を124.8 MHzで割った値であり、実機測定ではない。

| 診断 | キャッシュなし cycles | 4 KiB cycles | 4 KiB時間 µs | 同一周波数の速度比 |
| --- | ---: | ---: | ---: | ---: |
| 依存ALUループ | 11,783 | 6,712 | 53.782 | 1.755倍 |
| 条件分岐パターン | 12,809 | 8,988 | 72.019 | 1.425倍 |
| 関数呼出し・復帰 | 10,761 | 7,192 | 57.628 | 1.496倍 |
| load/store依存 | 7,689 | 6,166 | 49.407 | 1.247倍 |
| 3 KiB容量検査 | 12,431 | 8,580 | 68.750 | 1.449倍 |
| 6 KiB容量検査 | 12,381 | 20,184 | 161.731 | 0.613倍 |
| 自己書換え・`FENCE.I` | 365 | 1,490 | 11.939 | 0.245倍 |

最初の4本の速度比の幾何平均は **1.470倍**、実行時間にして約32.0%減。
起動初期化を含めると **1.417倍**相当。4 KiB版のfirst commitは274/275クロック。
タイミングを成立させるための段追加による待ちも、これらの数値に含まれる。

**全プログラムで速いわけではない。** 4 KiBに収まらない6 KiBループは
キャッシュなしより約63.0%時間が長く、`FENCE.I`を繰り返す短い処理もscrubの影響が大きい。
機能モデルの8 KiB版は6 KiB検査を10,856 cyclesで実行するが、8 KiB版の
124.8 MHzタイミングは今回の合格結果に含まれない。

## 実機での結果

対象FPGAのIDCODEは `0x81113043`。WindowsのFT2232H A（device index 0）を
既存の `windows_d2xx_xvc.py` で開き、WSLから `127.0.0.1:3721` へ接続した。
JTAGは6 MHz。CPU 124.8 MHz／メモリ62.4 MHzのPLL設定とbitstreamを維持した。

| 実機テスト | 結果 |
| --- | ---: |
| 基本命令・CSR・counter上位32-bit | 8/8 |
| 性能診断 | 4/4 |
| キャッシュ容量・自己書換えと`FENCE.I` | 3/3 |
| ACT4 RV64I | 51/51 |
| ACT4 RV64Zca | 32/32 |
| 合計 | **98/98** |

各ELFでPLL lock、JTAGコマンド受理、`tohost=1`、11個の64-bit counterの読出しと
整合性を検査した。ELFのハッシュも以前のモデル検証の98本と一致した。
`performance.elf` はcounter上位32-bitの固有パターン
`[16,8,1,2,3,4,5,6,7,8,9]` を検査した。

次は初回実機測定。`mcycle` はCPU起動・キャッシュ初期化から終端storeの応答を
受けてcounterを凍結するまでを含む。前節のfirst commit起点のRTL区間とは集計範囲が異なる。
JTAGによるロード時間は含まない。時間はcounterをPLL設定124.8 MHzで割った換算値で、
外部計測器による発振周波数の測定は行っていない。

| 診断 | 実機mcycle | 実機minstret | 換算時間 µs |
| --- | ---: | ---: | ---: |
| 依存ALUループ | 6,989 | 4,616 | 56.002 |
| 条件分岐パターン | 9,264 | 2,825 | 74.231 |
| 関数呼出し・復帰 | 7,473 | 1,546 | 59.880 |
| load/store依存 | 6,451 | 1,290 | 51.691 |
| 3 KiB容量検査 | 8,865 | 6,174 | 71.034 |
| 6 KiB容量検査 | 20,509 | 6,169 | 164.335 |
| 自己書換え・`FENCE.I` | 1,810 | 72 | 14.503 |

既定のbitstream・書込み幅によるSRAM再書込み後も基本8本が再度合格した。
性能・キャッシュ診断7本はさらに3回ずつ合格し、初回と合わせた4回のmcycleの
ばらつきは各診断で最大1 cycleだった。
[反復値](../toolchain/measurements/2026-09-05-rv64-124p8-hardware/repeated-mcycle.json)も保存した。

同じ最終RTLのキャッシュなし版との実機A/Bは未実施なので、**1.470倍は引き続き
モデルでの比較値**とする。温度・電圧を変えた余裕の検証も今回の範囲には含めない。

### JTAGロードで見つかった不具合

最初の8-byte WRITE64ではALUテストが完了しなかった。再書込みでも再現し、
`addi x31,x0,0x5a` が `0x9a` として現れる診断を得た。
同じ48通りの即値・配置位置の検査は、現bitstreamの1-byte書込みと、
過去の実績あるbitstreamの8-byte書込みで48/48合格した。
JTAGクロックだけを1 MHzに下げても現版の8-byte書込みは改善しなかった。

したがって実機合格は1-byte書込みに限定する。8-byte転送経路の根本原因は未確定。
既存STAで未制約のJTAG domainにCPU側の+64 ps/+290 psを適用してはならない。
全ボードの論理マッピング後モデルによる追加確認は、Struo/Celoxアダプタの
`NonScalarControl(5005)` でシミュレーション開始前に止まった。
このモデル検査を合格やCPU命令の不合格として数えない。

[実機ログ・コマンド・ハッシュ](../toolchain/measurements/2026-09-05-rv64-124p8-hardware/README.md)と
[書込み不具合の再現手順](../toolchain/mre/rv64-jtag-write64/README.md)を保存した。

## 検証、成果物、再現

- Veryl check / fmt検査、Rust **140項目**合格。
- 通常モデル：98 ELFs × TAGE / bimodal / static = **294実行**合格。
- ボード相当モデル：98 ELFs × 2エッジ順序 × キャッシュ0/2/4/8 KiB = **784実行**合格。
- 合成・マッピングの等価性合格。キャッシュはEBRへマッピングしたモデルでも
  補充・無効化を検査した。Coreを含む全体の配置配線後機能シミュレーションは未実施。
- コメント整備後に同じ条件で配置配線を再実行し、checkpoint全体のSHA-256が一致した。
- bitstream生成成功。Windows D2XX/XVC経由の実機検証は上記98/98合格。
  以前のLinux直接接続での `failed to initialize ftdi` は、Windows側USBの制御を確立して解消した。

成果物は以下。大きなbuildファイルはgit管理外で、検査結果とハッシュを証拠フォルダへ保存した。

- [配線済みcheckpoint](build/riscv-hardware-cached-4k-124p8-20260905-ce8.texo.json)
- [bitstream](build/riscv-hardware-cached-4k-124p8-20260905-ce8.bit)
- [測定・検査結果](../toolchain/measurements/2026-09-05-rv64-124p8/README.md)
- [実機検証結果](../toolchain/measurements/2026-09-05-rv64-124p8-hardware/README.md)

bitstream SHA-256:
`cbb8505df08ae2f0e4183278fa46673609adea3a9e35206539ab84bc9b2c0f7f`

リポジトリルートから再現する。保存したコマンドはPLL周波数を変更せず、
合成目標・デバイス・CE分岐条件を含む。コマンド終了コードだけでSTA合格を判断しない。

```sh
python3 - <<'PY'
import json, pathlib, subprocess
p = pathlib.Path('toolchain/measurements/2026-09-05-rv64-124p8/reproduce-command.json')
command = json.loads(p.read_text())
subprocess.run(command, check=True)
checkpoint = pathlib.Path(command[command.index('--output') + 1])
timing = json.loads(checkpoint.read_text())['timing']
assert timing['met_timing'] and timing['worst_slack_ps'] >= 0
assert timing['worst_hold_slack_ps'] >= 0
assert all(x['cell'].startswith('ff_board.jtag_debug_transport.')
           for x in timing['unchecked_endpoints'])
subprocess.run(['target/texo-git/bin/texo', 'bitgen', str(checkpoint),
                '--bit', 'riscv/build/riscv-hardware-cached-4k-124p8-reproduce.bit'], check=True)
PY
```

## 次の改良

1. 8-byte JTAG転送を根本修正する。payloadの読戻し・段ごとの照合で破損箇所を特定し、
   JTAGクロックのsetup/hold検査を追加する。修正版はCPU 124.8 MHzを維持し、
   再度STA、転送診断、98 ELFsと11カウンタを通す。それまでは1-byte書込みを使用する。
2. キャッシュミス時の待ちを短縮する。連続行の取得・補充を改善し、
   容量超過での0.613倍という弱点を解消する。124.8 MHzは維持する。
3. 8 KiB版も同じ条件でタイミングを閉じ、大きなコードの利点とscrub時間を比較する。
4. setup余裕の拡大、データ待ち、小型分岐先予測を順に評価する。
   各変更はSTAと実行時間の両方を通過条件とし、命令発行幅の拡大はその後に判断する。
