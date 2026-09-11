# media-process-plugin — 加工: 顔ぼかし / スタンプ(スロット `composer.media`、契約 v0.5、プラグイン v0.1.0)

コンポーザーの添付画像(PNG / JPEG)に **顔検知ぼかし** と **スタンプ(オブジェクト)** を掛けるサンプル。
UI はホストの**専用全画面エディター**(中央プレビュー + 右の「プラグイン / ジャンル」列 + 下付きの編集ボタン列 + キャンセル / 適用)で、
プラグインは `media.catalog`(セッション無し)で「ジャンル / アクションの一覧」を返し、プレビューは `composer.media.preview`(v0.5: `width` / `height` 付き)で差し替え、
プレビューの上には `composer.media.overlay`(v0.5)で **顔の枠 / オブジェクトの枠 / ハンドル / ボタン** を重ねる。
ジャンルは契約の**既定 id** `process`(ラベル / アイコンはホストが持つので付けない)。
契約は [`../CONTRACT.md`](../CONTRACT.md) §2.2(`media.*` / `composer.media.preview` / `composer.media.overlay`)と §3(`Box` / `Icon` / `Image` の `data:` URI / `on: {tap, drag}`)。

| 入口 | 中身 |
|---|---|
| `plugin.manifest` | `{id: sample.media-process, name: 加工(顔ぼかし / スタンプ)(サンプル), version: 0.1.0, kind: action, slots: [composer.media], permissions: [fow.widget]}` |
| `media.catalog {}` | 静的な一覧 `{genres: [{id:"process", actions:[faces / sticker / reset]}]}`。セッション不要 |
| `media.edit {token, index, name, mimeType, bytesB64}` | デコード → 保持(長辺 ≤ 2048)→ 初期プレビューを push(長辺 ≤ 360、`width`/`height` 付き)→ 戻り `{genres}` |
| `media.action {token, genre:"process", action:"faces"}` | wasm 内で顔検知 → 顔ごとにボックスぼかし → プレビュー + 重ね(顔の枠)。顔が無ければ `toast("顔が見つかりませんでした")` |
| `media.action {…, action:"sticker"}` | 下付き FOW `sticker-pick`(3 つのスプライトを `Image` で並べる)を開く |
| `media.action {…, action:"reset"}` | 顔のトグルとオブジェクトを捨て、重ねを消し、元のプレビューに戻す |
| `face.<i>`(重ねの `tap`) | 顔 `i` のぼかしを on / off → プレビュー + 重ねを出し直す |
| `sticker.add.<i>`(FOW の `tap`) | スプライト `i` をプレビュー中央に置く(一辺 = 短辺 / 3、角度 0)→ 選択 → プレビュー + 重ね |
| `obj.<i>.move`(`drag` / `tap`) | ドラッグで移動(`dx`/`dy` を中心に足す。中心はプレビュー内に clamp)/ タップで選択 |
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
                                              Box k:"obj.0"        on:{drag:"obj.0.move", tap:"obj.0.move"}   … 回転後の外接矩形(選択中は橙の枠)
                                              Box k:"obj.0.scale"  on:{drag:"obj.0.scale"}                      … 右下の丸いハンドル 20×20
                                              Box k:"obj.0.rotate" on:{drag:"obj.0.rotate"}                     … 上(回転に追従)の青いハンドル 20×20
                                              Box k:"obj.0.delete" on:{tap:"obj.0.delete"} c:[Icon "delete"]     … 枠の下のボタン 26×26
                                              Box k:"obj.0.dup"    on:{tap:"obj.0.dup"}    c:[Icon "content_copy"] ]
  call("obj.0.move",   {phase:"update", x, y, dx, dy}) ▶ 中心 += (dx, dy)             ◀ preview + overlay
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
- 正面顔モデル `model/seeta_fd_frontal_v1.0.bin`(約 1.2 MB、rustface リポジトリのもの)を `include_bytes!` で埋め込み、初回の `faces` で
  `rustface::read_model` にパースして保持する(ファイル I/O 無し)。**正面顔向き**なので横顔 / 強い傾きは拾いにくい。
- 入力は長辺 **480** に縮めたグレースケール(`src/faces.rs`)。カスケードの最小窓が 20px なので、480px で画像の 1/24 より小さい顔は拾えない。
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
- 重ねの `Box` は回転後の**軸平行な外接矩形**(ホストの `Box` は回転できない)。ドラッグの payload `{phase, x, y, dx, dy}` は `dx`/`dy` だけ使い、
  拡縮 / 回転はハンドルの位置を `start` から積算して中心との相対で計算する(拡縮は射影なので中心を越えても最小 16px で止まる)。
- 重ねは顔もオブジェクトも無くなったら `root: null` で消す。プレビューを差し替えるたびに重ねも出し直す(座標系は同じだが枠の位置が変わるため)。

## 制限 / メモ

- プレビューは `composer.media.preview` の `bytesB64` に **PNG の生 base64**(`data:` URI ではない)で送る。長辺 ≤ 360px、`width`/`height` 付き。
- 結果は `media.commit` の**戻り値**で返す(`composer.media.replace` は使わない)。`token` が一致しない `media.action` / `media.commit` は `D01`。
- `mimeType` が PNG / JPEG 以外は `C01`、壊れた base64 / 画像、無い index、不明な操作は `D01`(panic しない)。未知の関数は `C01`。
- wasm はインタープリタで動くので、顔検知は 480px(再検知 960px)、適用は長辺 2048px に丸めて処理量を抑えている。
- wasm サイズは約 1.8 MB(うち 1.2 MB がモデル。`opt-level = "s"` / LTO / strip)。

## ビルド / テスト

```sh
cargo test -p media-process-plugin                                             # native 単体テスト(ops / sprites / 検知器のロード + call の end-to-end)
cargo build --release --target wasm32-unknown-unknown -p media-process-plugin  # → target/wasm32-unknown-unknown/release/media_process_plugin.wasm(約 1.8 MB)
```

## 端末で試す(rasq-client)

1. サイドバー → **WASMプラグイン試験 (dev)** → `.wasm のパスからインポート` に `media_process_plugin.wasm` の絶対パスを入れて **読み込み**
2. **スロットに登録**(権限 `fow.widget` を許可)
3. コンポーザーで画像(PNG / JPEG)を添付 → 添付タイルの **… → プラグインで加工** → 「加工(顔ぼかし / スタンプ)(サンプル)」でエディターが開く
4. 下の列で **顔ぼかし**(顔の枠をタップで個別に on / off)/ **スタンプ**(下付き FOW から貼る → 枠をドラッグで移動、右下で拡縮、上で回転、下のボタンで削除 / 複製)/ **リセット**
5. ヘッダーの **適用** で添付が差し替わる(**キャンセル** で破棄)

## 顔ぼかしについて(2026-09-11)

実機(wasm インタプリタ上の rustface)では画面の 40% 程度の顔でも検知できなかったため、`media.catalog` の一覧から
`faces` を外した(`media.action {action:"faces"}` を直接呼べば従来どおり動く)。ホストの `ml.infer`(ONNX)がつながったら
YuNet で置き換える。基本の加工(ぼかし / モザイク)はホスト側の pro_image_editor(D-173)が担う。

---

## このリポジトリの形

- クレートはルート(`src/`)、共通部品 [`rasq-plugin-kit`](kit) は `kit/` に同梱(開発用モノレポから写している)。
- 契約は [`CONTRACT.md`](CONTRACT.md)(スロットプラグイン契約 v0.1)。
- ビルド済み `.wasm` は [`dist/`](dist)。`cargo build --release --target wasm32-unknown-unknown` で更新。
