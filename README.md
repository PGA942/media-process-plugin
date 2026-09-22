# media-process-plugin — 加工: 顔ぼかし / スタンプ(スロット `composer.media`、契約 v0.13)

コンポーザーの添付画像(PNG / JPEG)に **顔検知ぼかし** と **スタンプ(オブジェクト)** を掛けるサンプル。
UI はホストの加工画面に重なる**編集パネル**(中央プレビュー + 下付きの横リスト + キャンセル / 適用)で、
プラグインは `media.edit` で「ジャンル / アクションの一覧」を返し、プレビューは `composer.media.preview`(v0.5: `width` / `height` 付き)で差し替え、
プレビューの上には `composer.media.overlay`(v0.5)で **顔の枠 / オブジェクトの枠 / ハンドル / ボタン** を重ねる。
ジャンルは契約の**既定 id** `process`(ラベル / アイコンはホストが持つので付けない)。

- id `sample.media-process` / kind `action` / slots `["composer.media"]` / permissions `["fow.widget"]`
- **下段バーのボタン宣言(v0.10)**: `manifest.json` の `actions` に
  `{"id": "process", "slot": "composer.media", "label": "加工", "icon": "auto_fix_high", "tags": ["Filter", "Blur"]}` を 1 つ。
  `tags` で内蔵の「フィルター」「ぼかし」とバンドルされる。`accepts` 省略 = `["image/*"]`、`output` 省略 = `replace`。
  宣言した `id` と同じ id の編集はジャンルに無いので、ボタンを押すと編集パネルが開くだけ(近道は無い)。
  押したときの `media.edit` には `actionId: "process"` が付くが、返すジャンル一覧は変わらない。
- 正典は [`CONTRACT.md`](CONTRACT.md) が指す `rasq-sdk-sample/docs/slots.md` §2.2(`media.*` /
  `composer.media.preview` / `composer.media.overlay`)と §3(`Box` / `Icon` / `Image` の `data:` URI / `on: {tap, drag}`)。

## このリポジトリの形(`rasq` ワークスペース)

```
rasq.toml            schema = "rasq.workspace/1" / [author] namespace = "sample"
                     [[plugins]] name = "media-process-plugin" / slot = "composer.media" / id = "sample.media-process"
                     [[models]]  models/seeta-fd-frontal.model.json(顔検知のカスケードの宣言)
                     [[assets]]  models/seeta_fd_frontal_v1.0.bin(その本体、kind = data)
Cargo.toml           rasq CLI が rasq.toml から作り直す(手で直さない)
kit/                 rasq-sdk-sample の rust/rasq-plugin-kit の写し
models/              顔検知モデル(SeetaFace FuSt 正面顔カスケード、約 1.2 MB)+ その model.json
plugins/media-process-plugin/
  Cargo.toml         crate-type = ["cdylib", "rlib"] / rasq-plugin-kit = { path = "../../kit" }
  manifest.json      plugin.manifest の答えそのもの(ボタン宣言もここ)
  src/lib.rs         include_str!("../manifest.json") + handle の match + #[cfg(test)]
  src/faces.rs       rustface(SeetaFace FuSt)での顔検知。モデルは ../../../models/… を include_bytes!
  src/ops.rs         矩形 / ぼかし / スタンプ合成
  src/sprites.rs     手続き的なスプライト 3 種
  tests/fixtures/    rasq call 用の画像
dist/                rasq build --raw --no-sign の出力(.rasqpkg と生の .wasm)
```

マニフェストは **`plugins/media-process-plugin/manifest.json` の 1 か所**。`src/lib.rs` は `include_str!` で
それをそのまま返すので、`rasq check` の往復検査(`plugin.manifest` ⇔ `manifest.json`)が常に一致する。

### モデルの宣言(`docs/cli.md` §2 / §4)

顔検知のカスケードは `rasq.toml` に 2 つに分けて書いてある。

- `[[models]] manifest = "models/seeta-fd-frontal.model.json"` — **どんなモデルを使っているか**の宣言。
  `rasq inspect` の `model:` 行に出るので、審査でモデルの出所 / ライセンス / SHA-512 が見える。
- `[[assets]] path = "models/seeta_fd_frontal_v1.0.bin" kind = "data"` — **その本体**。配布物に 1 つだけ入る。
  今はビルド時に `include_bytes!` で wasm に取り込んでいるが、ホストが `storage.get({ns: "asset"})` を
  配線したら `include_bytes!` をやめて wasm を 1.2 MB 縮められる。

> モデル本体を `[[models]]` 側(`runtimes.*.file`)に書くと `rasq build` は入れてくれるが、`rasq inspect` が
> 「目次に載っていないファイル」として **exit 1** で落ちる(SDK の不備。コードのコメントは「警告にとどめる」と
> 言っているのに `problems` に積んでいる)。そのため本体は `[[assets]]` 側で宣言している。
> また `model-manifest.schema.json` v0.1 の `runtimes` は `onnx` / `llama` / `server` しか持たず、
> 「プラグインの wasm に取り込む実体」を表せないので、`runtimes` のキーは `embedded` にしてある。

## 契約(`docs/slots.md` §2.2 `composer.media`)

| 入口 | 中身 |
|---|---|
| `plugin.manifest` | `manifest.json` そのもの(`{id: sample.media-process, name: 加工(顔ぼかし / スタンプ)(サンプル), version: 0.1.0, kind: action, slots: [composer.media], permissions: [fow.widget], actions: [{id: process, slot: composer.media, …}]}`) |
| `media.catalog {}` | 静的な一覧 `{genres: [{id:"process", actions:[faces / sticker / reset]}]}`。セッション不要 |
| `media.edit {token, index, name, mimeType, actionId?, bytesB64}` | デコード → 保持(長辺 ≤ 2048)→ 初期プレビューを push(長辺 ≤ 360、`width`/`height` 付き)→ 戻り `{genres}` |
| `media.action {token, genre:"process", action:"faces"}` | wasm 内で顔検知 → 顔ごとにボックスぼかし → プレビュー + 重ね(顔の枠)。顔が無ければ `toast("顔が見つかりませんでした")` |
| `media.action {…, action:"sticker"}` | 下付き FOW `sticker-pick`(3 つのスプライトを `Image` で並べる)を開く |
| `media.action {…, action:"reset"}` | 顔のトグルとオブジェクトを捨て、重ねを消し、元のプレビューに戻す |
| `face.<i>`(重ねの `tap`) | 顔 `i` のぼかしを on / off → プレビュー + 重ねを出し直す |
| `sticker.add.<i>`(FOW の `tap`) | スプライト `i` をプレビュー中央に置く(一辺 = 短辺 / 3、角度 0)→ 選択 → プレビュー + 重ね |
| `obj.<i>.move`(`drag` / `tap`) | ドラッグで移動(`dx`/`dy` を中心に足す。中心はプレビュー内に clamp)/ タップで選択。`end` の `snap`(v0.6)で合わせる先に収める(下記) |
| `obj.<i>.scale`(`drag`) | 右下ハンドル。中心→ハンドル方向への射影の比で一辺を変える(**縦横比 1:1 固定、最小 16px**) |
| `obj.<i>.rotate`(`drag`) | 上のハンドル。中心からの `atan2` で角度(ハンドルは回転に追従する) |
| `obj.<i>.delete` / `obj.<i>.dup`(`tap`) | 削除 / 複製(+16px ずらして選択) |
| `closeSticker`(`tap`)/ `stickerClosed` | FOW を閉じる / ホスト側で閉じられた(画像は保持) |
| `media.commit {token}` | フル解像度で ぼかし(有効な顔)→ スタンプ合成 → PNG → **戻り値で** `{bytesB64, mimeType:"image/png", name:"<元名>.png"}`。重ねを消し FOW を閉じ、状態を捨てる |
| `media.cancel {token}` | 重ねを消し FOW を閉じ、状態を捨てる |

## 流れ

```
ホスト                                   プラグイン
  call("plugin.manifest", {}) ─────────▶ manifest(id: sample.media-process, slots: [composer.media], permissions: [fow.widget])
  call("media.catalog", {}) ───────────▶ ◀ 戻り {genres: [{id:"process", actions:[
                                              {id:"faces",   label:"顔ぼかし", icon:"face_retouching_off"},
                                              {id:"sticker", label:"スタンプ", icon:"emoji_emotions"},
                                              {id:"reset",   label:"リセット", icon:"restart_alt"} ]}]}
  call("media.edit", {token, name, mimeType, bytesB64, …}) ▶ デコード → 保持(長辺 ≤ 2048)
                                        ◀ composer.media.preview {token, bytesB64(PNG), mimeType, width, height}   ◀ 戻り {genres}
  call("media.action", {token, genre:"process", action:"faces"}) ▶ 長辺 480 のグレースケールで検知(無ければ 960 で再検知)
                                        ◀ composer.media.preview(顔をぼかしたプレビュー)
                                        ◀ composer.media.overlay {token, root: Stack(width, height)[
                                              Box k:"face.0" (left/top/width/height, borderColor 緑) on:{tap:"face.0"}, … ]}
                                        (顔が無ければ ◀ toast {message:"顔が見つかりませんでした"} だけ)      ◀ 戻り {}
  call("face.0", {kind:"tap"}) ─────────▶ 顔 0 のぼかしを off(枠は白の細線に)  ◀ preview + overlay
  call("media.action", {token, genre:"process", action:"sticker"}) ▶
                                        ◀ fow.open("sticker-pick", title:"スタンプ", height 0.28)
                                              root: Row[ Column[Image k:"sticker.0" src:"data:image/png;base64,…" 64×64 on:{tap:"sticker.add.0"}, Text], ×3 ] + Button 閉じる(closeSticker)
  call("sticker.add.1", {kind:"tap"}) ───▶ 中央に一辺 = 短辺/3 のオブジェクト、選択
                                        ◀ composer.media.preview(スタンプを合成したプレビュー)
                                        ◀ composer.media.overlay root: Stack[
                                              Box k:"obj.0" p:{label:"スタンプ 1", angle:0, …} on:{drag:"obj.0.move", tap:"obj.0.move"} … 回転前の正方形(選択中は橙の枠。ホストが angle で回して描く)
                                              Box k:"obj.0.scale"  on:{drag:"obj.0.scale"}                      … 右下の丸いハンドル 20×20
                                              Box k:"obj.0.rotate" on:{drag:"obj.0.rotate"}                     … 上(回転に追従)の青いハンドル 20×20
                                              Box k:"obj.0.delete" on:{tap:"obj.0.delete"} c:[Icon "delete"]     … 枠の下のボタン 26×26
                                              Box k:"obj.0.dup"    on:{tap:"obj.0.dup"}    c:[Icon "content_copy"] ]
  call("obj.0.move",   {phase:"update", x, y, dx, dy}) ▶ 中心 += (dx, dy)             ◀ preview + overlay
  call("obj.0.move",   {phase:"end", …, snap:{key, left, top, width, height, angle}}) ▶ 合わせる先の内側に 1:1 で収める + 角度  ◀ preview + overlay
  call("obj.0.scale",  {phase:"start"|"update"|"end", dx, dy}) ▶ 一辺 = 開始時の一辺 × 射影 / 開始距離(≥ 16)  ◀ preview + overlay
  call("obj.0.rotate", {phase, dx, dy}) ▶ 角度 = atan2(ハンドル − 中心) + π/2                ◀ preview + overlay
  call("obj.0.dup",    {kind:"tap"}) ────▶ obj.1 を +16px に作って選択                     ◀ preview + overlay
  call("obj.0.delete", {kind:"tap"}) ────▶ 消す(残りは詰めて番号が変わる)                  ◀ preview + overlay(何も無ければ root: null)
  call("media.commit", {token}) ─────────▶ フル解像度に ぼかし(有効な顔)→ スタンプ(座標 × フル/プレビュー)→ PNG
                                        ◀ composer.media.overlay {root: null}  ◀ fow.close("sticker-pick")
                                        ◀ 戻り {bytesB64, mimeType:"image/png", name:"<元名>.png"}  → 状態を破棄
  call("media.cancel", {token}) ─────────▶ ◀ overlay null / fow.close  ◀ 戻り {}  → 状態を破棄
```

## 顔検知について(YuNet / ONNX ではない)

- ホスト(rasq-client)には **ONNX ランタイムが無い**ので YuNet 等の DNN モデルは動かせない。代わりに **SeetaFace の FuSt カスケード**(LAB boosted + SURF MLP)の純 Rust 移植
  [`rustface`](https://crates.io/crates/rustface)(BSD-2-Clause、`default-features = false` = rayon 無し)を **wasm の中で**動かす。
- 正面顔モデル `models/seeta_fd_frontal_v1.0.bin`(約 1.2 MB、rustface リポジトリのもの)を `include_bytes!` で埋め込み、初回の `faces` で
  `rustface::read_model` にパースして保持する(ファイル I/O 無し)。**正面顔向き**なので横顔 / 強い傾きは拾いにくい。
- 入力は長辺 **480** に縮めたグレースケール(`plugins/media-process-plugin/src/faces.rs`)。カスケードの最小窓が 20px なので、480px で画像の 1/24 より小さい顔は拾えない。
  そのため **1 回目で顔が無ければ長辺 960 で 1 回だけ再検知**する(集合写真向け。処理量は約 4 倍)。スコア閾値は 2.0(既定 3.85 より拾いやすく。誤検知は枠のタップで外せる)。
- 検知した矩形は 25% 広げて(髪 / 顎)フル画像 px で保持し、プレビュー / フル画像へは比率で写す(`ops::RectF::map`)。ぼかしは矩形の中だけの
  分離型ボックスぼかし 2 パス、半径 = 矩形の短辺 / 8(最低 2px)なのでプレビューでもフル画像でも見え方が揃う(`ops::blur_rect`)。
- 参考(native, release): 1666×1136 の 29 人の集合写真 → 480px では 0(顔 ≈ 16px)、960px では十数人、1024px で 26 人、原寸で 29 人 / 約 0.3 秒。
  実写での動作確認は `FACE_FIXTURE=/path/to/photo.jpg cargo test -p media-process-plugin --release -- --ignored real_photo --nocapture`。

## スタンプ / 重ねについて

- スプライトは 3 つ(スマイル / ハート / スター)、96×96 RGBA を **数式だけで**描く(`src/sprites.rs`、3×3 スーパーサンプリング、フォント無し)。
  FOW には PNG の `data:` URI(`Image.p.src`)で出す。
- オブジェクト(`ops::Sticker` = スプライト番号 / 中心 / 一辺 / 角度)は**プレビュー px** で持つ(= 重ねの座標系)。プレビューへの合成は
  逆写像 + バイリニア + アルファ合成(`ops::composite_sprite`)、適用時は座標 × (フル / プレビュー) で同じ関数をフル画像に掛ける。
- 重ねの `Box` は**回転前の正方形**(中心 ± 一辺/2)に `p.angle`(度)を付けて出す(v0.6: ホストが中心で回転して描く)。ハンドル / ボタンの
  位置だけは回転後の軸平行な外接矩形(`ops::Sticker::bbox`)から計算する。ドラッグの payload `{phase, x, y, dx, dy}` は `dx`/`dy` だけ使い、
  拡縮 / 回転はハンドルの位置を `start` から積算して中心との相対で計算する(拡縮は射影なので中心を越えても最小 16px で止まる)。
- **合わせる先(v0.6)**: 重ねの `Box` には一覧用の `p.label`(オブジェクト = 「スタンプ 1」、顔 = 「顔 1」。ハンドル / ボタンには付けない)を
  付ける。ホストが合わせる先を決めていると `obj.<i>.move` の `end` に `snap: {key, label?, left, top, width, height, angle}`(プレビュー px / 度)が
  付くので、スタンプを**その矩形の内側に 1:1 で収める**: 一辺 = `min(width, height)`(最小 16px)、中心 = 矩形の中心、角度 = `angle`(省略時 0)。
  `key` が自分の枠(`obj.<i>`)なら無視。拡縮 / 回転ハンドルの `end` に付いた `snap` は使わない(`ops::Sticker::fit_into`)。
- 重ねは顔もオブジェクトも無くなったら `root: null` で消す。プレビューを差し替えるたびに重ねも出し直す(座標系は同じだが枠の位置が変わるため)。

## 制限 / メモ

- プレビューは `composer.media.preview` の `bytesB64` に **PNG の生 base64**(`data:` URI ではない)で送る。長辺 ≤ 360px、`width`/`height` 付き。
- 結果は `media.commit` の**戻り値**で返す(`composer.media.replace` は使わない)。`token` が一致しない `media.action` / `media.commit` は `D01`。
- `mimeType` が PNG / JPEG 以外は `C01`、壊れた base64 / 画像、無い index、不明な操作は `D01`(panic しない)。未知の関数は `C01`。
- wasm はインタープリタで動くので、顔検知は 480px(再検知 960px)、適用は長辺 2048px に丸めて処理量を抑えている。
- wasm サイズは約 1.8 MB(うち 1.2 MB がモデル。`opt-level = "s"` / LTO / strip)。

## ビルドと確認

`rasq` CLI(`rasq-sdk-sample/rust/rasq-cli`)をパスに置いて、リポジトリのルートで:

```sh
rasq check                 # wasm32 ビルド → export 検査 → plugin.manifest 往復 → スキーマ / 登録簿 → cargo test
rasq test                  # cargo test だけ(ops / sprites / 検知器のロード + call の end-to-end、22 本)
rasq build --raw --no-sign # dist/sample.media-process-0.1.0.rasqpkg と dist/media_process_plugin.wasm
rasq inspect dist/sample.media-process-0.1.0.rasqpkg
```

```
$ rasq check
$ cargo build --release --target wasm32-unknown-unknown
plugins/media-process-plugin:
  ok: target/wasm32-unknown-unknown/release/media_process_plugin.wasm (1798 KB)
1 targets, 0 errors, 0 warnings
$ cargo test
... test result: ok. 22 passed; 0 failed; 1 ignored

$ rasq inspect dist/sample.media-process-0.1.0.rasqpkg
bundle: rasq.bundle/1 sdk 0.1.0
root:   sample.media-process 0.1.0 (action)
asset:  models/seeta_fd_frontal_v1.0.bin (data)
model:  models/seeta-fd-frontal.model.json lazy=false
files:  bundle.json / manifest.json / media_process_plugin.wasm /
        models/seeta_fd_frontal_v1.0.bin / models/seeta-fd-frontal.model.json / SHA512SUMS
目次と中身: ok
```

`cargo test` の `real_photo` は `#[ignore]`。手元の写真で検知を見たいときは
`FACE_FIXTURE=/path/to/photo.png cargo test -p media-process-plugin -- --ignored real_photo`。

### `rasq call` — wasm をそのまま叩く

`--args` の値の `"@b64:<path>"` はそのファイルの base64 に置き換わる
(fixture: `plugins/media-process-plugin/tests/fixtures/sample-320x240.png`、320×240)。

```sh
rasq call media-process-plugin plugin.manifest   # manifest.json と一致する
rasq call media-process-plugin media.catalog     # {genres: [{id:"process", actions:[sticker/reset]}]}

cat > /tmp/edit.json <<'JSON'
{"token":"","index":0,"name":"sample.png","mimeType":"image/png","actionId":"process",
 "width":320,"height":240,
 "bytesB64":"@b64:plugins/media-process-plugin/tests/fixtures/sample-320x240.png"}
JSON
rasq call media-process-plugin media.edit --args @/tmp/edit.json --then media.commit

rasq call media-process-plugin face.0            # セッション無し → D01(exit 1)
```

| call | 戻り / ホスト関数 |
|---|---|
| `plugin.manifest` | `manifest.json` そのもの |
| `media.catalog` | `{"genres":[{"id":"process","actions":[{"id":"sticker",…},{"id":"reset",…}]}]}` |
| `media.edit`(`actionId: "process"` 付き) | `{"genres":…}` / `composer.media.preview {token, bytesB64(PNG), mimeType:"image/png", width:320, height:240}` |
| `media.commit` | `{"bytesB64":"…", "mimeType":"image/png", "name":"sample.png"}` |
| `face.0`(セッション無し) | `D01 "no image (call media.edit first)"`(`rasq call` は exit 1) |

> `media.action` / `face.*` / `sticker.add.*` / `obj.*` は args が要るが、`rasq call --then` は args が `{}` 固定なので
> 1 回の `rasq call` では続けて叩けない(SDK 側の制限)。埋め込みモデルの読み込み + 顔検知(`media.action {action:"faces"}`)は、
> 同じ wasm に一時の入口を足した使い捨てビルドで確かめてある(320×240 で約 170 ms、トラップ無し / 顔 0 件)。
> それ以外の経路は `cargo test` の end-to-end で見ている。

## 端末での試し方(rasq-client)

1. `rasq build --raw --no-sign` のあと、出力の最後の行が出す `dist/` の**絶対パス**を控える。
   サイドバー → **WASMプラグイン試験 (dev)** → `.wasm のパスからインポート` にそれを入れて **読み込み**
2. **スロットに登録**(権限 `fow.widget` を許可)
3. コンポーザーで画像(PNG / JPEG)を添付 → 添付タイルの **… → プラグインで加工** → 加工画面の**下段バー**に
   宣言したボタン「加工」(`auto_fix_high` アイコン)が出る。押すと編集パネルが重なり、下付きの横リストに
   スタンプ / リセットが並ぶ(v0.10 で「ジャンル」の見方は廃止)
4. 下の列で **顔ぼかし**(顔の枠をタップで個別に on / off)/ **スタンプ**(下付き FOW から貼る → 枠をドラッグで移動、右下で拡縮、上で回転、下のボタンで削除 / 複製)/ **リセット**
5. ヘッダーの **適用** で添付が差し替わる(**キャンセル** で破棄)

## 顔ぼかしについて(2026-09-11)

実機(wasm インタプリタ上の rustface)では画面の 40% 程度の顔でも検知できなかったため、`media.catalog` /
`media.edit` が返す一覧から `faces` を外した(`media.action {action:"faces"}` を直接呼べば従来どおり動く)。
ホストの `ml.infer`(ONNX)がつながったら YuNet で置き換える。基本の加工(ぼかし / モザイク)はホスト側の
pro_image_editor(D-173)が担う。**この版でも振る舞いは変えていない**(一覧から外れたままで、呼べば動く)。

## 掲載(審査)に出す

`rasq build` が出す `dist/sample.media-process-0.1.0.rasqpkg` が**そのまま掲載できる形**
(zstd tar + `bundle.json` `rasq.bundle/1` + `SHA512SUMS`)。

```sh
rasq publish --dry-run     # 出す前の確認
```

- 開発者鍵と `dist/*.sig.json` は**コミットしない**(`.gitignore` の `*.sig.json`)。CI は `--no-sign` で組む。
- 契約の正典は [`CONTRACT.md`](CONTRACT.md) が指す `rasq-sdk-sample/docs/slots.md`。
