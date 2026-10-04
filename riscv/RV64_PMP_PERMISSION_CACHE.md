# PMP権限キャッシュ

2026-10-04。対象は作業中の104/52 MHz U/PMPプロファイル（`platform-user.json`）。
このRTLは104/52 MHzで素のTexoフローによりタイミングを満たし、実機で269 ELFが合格した。

## 目的

U-mode/PMP導入後の最悪経路は `rx_pc_q → PMP比較器 → ex_frontend_block_q` で、
11.4 nsのうち8.4 nsが配線だった。全命令のPCを4エントリ分の33 bit上下限と毎サイクル
比較し、その結果がRX→EXのフロントエンド停止判定に入っていた。データ側は、特権に
関係なく全load/store/AMOが2サイクルの権限待ちを払っていた。

PMP設定と特権はCSR書込みやtrap/MRETで直列化されてからしか変わらない。そこで、
一様な4 KiBページの権限を1エントリずつ保持し、比較器を毎アクセスの経路から外す。

## 構造

`PmpPageUniform` は1ページを登録済みPMPスナップショットに照らして分類する。
重なるエントリがないか、最初に重なるエントリがページ全体を含むときに一様とし、
そのR/W/Xを返す。比較はページ番号の21 bitで行う（下限・上限ごとに大小と一致の2つ）。
ページ内にPMP境界がある場合は一様でなく、従来どおりアクセスごとの正確な判定を使う。

**命令側（1エントリ: 有効、特権、ページタグ、X）**

- FD→RXで `fd_q.pc[31:12]` とタグ・特権を照合し、結果を `rx_ipc_*` に登録する。
  RX→EXは登録済みの1 bitを取り込むだけになり、比較器はこの経路から消える。
- ミスしたらRXを1サイクル保持する。専用レジスタ `rx_pmp_address_q` と既存の
  端点レジスタから正確な判定（第1パーセル含む）とページ分類を行い、結果を登録して
  一様ならエントリを埋める。ページをまたぐ32 bit命令は常にミス扱い。
- M-modeで全エントリがOFFなら常に許可（従来の結果と同じ）。

**データ側（1エントリ: 有効、特権、ページタグ、R、W）**

- EXで、バイパスが選びうる3つのベースレジスタそれぞれについて `PmpCachedPageMatch` が
  ページ一致を判定し、既存の物理領域チェックと同じ選択信号で1つを選ぶ。ベース、実効
  アドレス、最終バイトが同じページ（4 GiB未満）にあるときだけヒットする。
- ヒットは最初の待ちedgeで専用FF `pmp_data_hit_q` に登録され、待ちは2サイクルから
  1サイクルになる。許可は別FFに取り込み、発行経路の既存の許可LUTで選ぶ。
- ミスは既存の2段の正確判定をそのまま使い、同じ段でページ分類してエントリを埋める。

**無効化**: PMP CSR（index 18以上）への書込みがCMにある間、両エントリを無効化する。
登録済み範囲が新しい値になるまでの3サイクル、照合と充填を止める。特権はタグに含む
ので、trap/MRET/MPRVによる無効化は不要。

## 検証

- `PmpPageUniform`: ネイティブとECP5マップ後の各6,000ケースで参照分類と一致。一様な
  ページでは中のアクセス計約19万件がバイト単位oracleとキャッシュ権限で一致。
- `PmpCachedPageMatch`: 両backend各20万ケースで実効アドレスによる定義と一致。上位bit、
  ページ越え、最終バイトのいずれかを壊した変異はすべて検出。
- `tests/rv64-u/pmp-cache.S`（新規、12 trap）: 特権タグ、PMP書込み直後のアクセス、
  符号付きオフセットのページ越え、境界で分割されたページ、U fetchのX剥奪、M fetch
  キャッシュとU fetch、ページをまたぐ命令、M-modeのロックによるW/X剥奪を確認する。
  旧RTLと新RTLの両方で合格。キャッシュを故意に壊した8変異のうち7つを意図した段で
  検出した。残る1つ（3サイクル待ちの除去）は、CSR直列化のリダイレクト遅延に隠れて
  命令の結果としては観測できない保険である。
- 既存のCPU検証: ネイティブ536実行、同時クロックU kernel、同時クロックのメモリ/原子/
  PMP 31件がすべて合格。同時クロックモデルでは269 ELFすべてが新旧両RTLで合格した。

## 結果

**実機（2026-10-04）**: 104/52 MHzのbitstream
`riscv/build/riscv-hardware-compact-user-104-pmpcache-20261004.bit`
（SHA-256 `0bbc34d9af50346c8f316df56f30fff1e00d973cf8b2695d426f922df168eb0b`）を
tapirでSRAMに書き、検証用の269 ELF（`pmp-cache.elf` を含む）が**全件合格**した。
U kernelはmcycle 232,879／minstret 66,924。同時クロックモデルとの比較は以下のとおり。

| 種類 | 本数 | 実機とモデル |
| --- | ---: | --- |
| 自前ELF | 64 | mcycle/minstret完全一致 |
| 自前ELF | 68 | 命令数一致、mcycleが1少ない |
| U kernel | 1 | 命令数一致、mcycleが4少ない |
| ACT4 | 136 | 実機の命令数が1少なく、mcycleが4〜5少ない |

1サイクル差は2:1クロックの既知の差（124.8/62.4 MHz認定でも±1）。ACT4の命令数は
84 MHz実機とネイティブモデルの値に一致するので、同時クロックモデル側がACT終了時に
1命令多く数えているとみられる（未確認）。

**タイミング**: 作業中の104/52 MHz条件（実測モデル、194 psガード、既存4例外、
setup最適化1800 s）で、シード配置・配線ヒント・LUT/FF対・CEファンアウト指定を使わない
素のフローを比べた。

| ツール | 旧RTL | 新RTL |
| --- | --- | --- |
| ピン留めTexo `97b2b049` | 配線収束せず | 配線収束せず |
| 上流Texo `43153ec` | 63/128回で2,280ネット残り | 56/128回で2,827ネット残り |
| 修正Texo（PR #100） | setup −342 ps／hold −43 ps、未達、2,148 s | **setup 0 ps／hold +1 ps、達成、452 s** |
| nextpnr-ecp5 0.6（3シード、CPU Fmax） | 96.9／82.1／93.3 MHz | 90.2／92.5／85.3 MHz |

新RTLの実装はチェック付きbitgenのJTCK監査（515 setup／515 hold一致）も通った。
各ツール1回ずつ（nextpnrは3シード）の比較で、配置のばらつきは評価していない。
nextpnrでは新旧とも最悪経路にPMP関連セルはなく、ロード/ストア発行判定、デコード、
フェッチ、予測照合が最悪で、遅延の8割以上が配線だった。

**Texoの修正**: 素のフローが配線できなかった原因はTexo側にあった。実測遅延モデルを
指定すると配置遅延予測器が無効になり、初期配置がRUDY面積調整とレジスタ制御を使わない
密度展開だけになって、タイルを容量いっぱいまで詰めていた。過剰使用は中央付近の
H02/V02/H06/V06に広く分布していた。タイミング重みがなくても混雑考慮の配置を使うよう
直すと、旧RTLでHPWLが272,141→200,275、4回目の過剰使用配線が9,696→102本になり、
初回配線が11回で収束した。修正は [Texo PR #100](https://github.com/fabrica-eda/texo/pull/100)
（Texo全510テスト合格）で、RicaのピンはそのコミットとStruo `824bfa6` に上げた
（Ricaワークスペース256テスト合格）。

**性能**（同時クロックモデル、退役命令数が新旧で同じ262 ELF、104/52 MHz）:
中央値−0.48%、幾何平均−2.2%、サイクル加重±0%。173本速く、27本遅く、62本同じ。
pointer_chase −16.2%、ビット操作系−11%、AMO −7%。最悪はcall_nested +3.2%で、
増分はすべて52 MHzメモリの待ちだった。D側ヒットで要求が1サイクル早く出ても、
メモリedgeの位相次第で待ちに吸収される。D側ヒットを止めた変種では269本中266本が
旧RTLと同一サイクル（U kernel +0.08%）で、命令側キャッシュ単体はほぼ性能中立。
U kernel全体は234,109→232,883サイクル（−0.5%）。メモリ待ちが約7.8万サイクルを
占め、旧来のPMP待ちは大半がそれに隠れていた。

**構造**（マップ後グラフ、同じ合成オプション）:

| 項目 | 旧 | 新 |
| --- | ---: | ---: |
| セル数 | 42,327 | 43,476 |
| LUT／CarrySlice／FF増分 | — | +529／+534／+86 |
| `ex_frontend_block_q` 入力錐（LUT/carry/元レジスタ） | 47／601／388 | 25／9／50 |
| `ex_pmp_fetch_fault_q` 入力錐 | 22／592／340 | 1／0／1 |
| `rx_pc_q[31:0]` ファンアウト（合計／最大） | 480／15 | 224／7 |
| 最大論理段数 | 12 | 12 |

新しい深い終点は単一FFの `pmp_data_hit_q`（11段）。最初の実装はページ比較を33 bitで
行い比較チェーンが34本増えたため、21 bitのページ番号比較に書き直した。隣接ページの
ヒットも一度実装したが、桁上がりで比較相手を選ぶ構造が1段深くなるため、同じページで
ページをまたがないアクセスだけをヒットにした。

## 再現

```sh
cargo test --release --locked --offline -p verify-riscv --test pmp
(cd riscv && veryl check)
python3 riscv/kernel/build-user.py   # pmp-cache.elf を含む

# 素のフロー（シード配置・配線ヒントなし）。測定モデルと194 psガードはtexo-measuredが付ける。
riscv/tools/texo-measured pnr riscv --top HardwareCompactUserTestTop \
  --device LFE5UM5G-85F --package CABGA381 --speed 8 \
  --lpf riscv/lfe5um5g-85f-evn.lpf --pll-binding riscv/pll-user.json \
  --synthesis-goal-mhz 104 --jtagg-prefix jtag \
  --clock-constraints riscv/jtck-clock-constraints.json \
  --timing-exceptions riscv/timing-coverage-exceptions.json \
  --architecture toolchain/worktrees/dsp/deps/ecp5-dsp-architecture-v6.txdb \
  --setup-optimization-budget-seconds 1800 --output build/user-104-pmpcache.txcp

# nextpnr参照用のネットリスト（Struoマップ、JTAG/PLL結線済み）
cargo run --release --locked -p verify-riscv --example nextpnr_export -- \
  riscv HardwareCompactUserTestTop riscv/pll-user.json 104 build/user-104.nextpnr.json
```

実装・監査・実機の記録は `toolchain/measurements/2026-10-04-rv64-u-104-pmpcache/`
（ソース、チェックポイント、bitgen監査、実機ログ、モデル比較）にある。検証・解析
スクリプトは `.agent-artifacts/pmp-permission-cache-20261004/` にある。
