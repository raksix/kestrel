# Kestrel — cross-platform yakalama, anotasyon ve paylaşım aracı

**Doküman türü:** Master plan / teknik spec
**Hedef:** ShareX'in özellik setini macOS, Windows ve Linux'ta karşılayan, native hisli, açık kaynak bir uygulama
**Stack:** Tauri v2 · Rust çekirdek · React + TypeScript arayüz
**Lisans:** GPL-3.0

---

## 0. Yönetici özeti

ShareX, Windows'a derinden bağlı (WinForms, GDI+, Win32 hook, registry, WinRT OCR) ~250k satırlık bir C#/.NET kod tabanı. Kod düzeyinde taşınabilir bir çekirdeği yok. Kestrel bu yüzden **kod portu değil, spec portu**: ShareX'in davranışını, veri formatlarını ve kullanıcı zihinsel modelini birebir koruyup altındaki her şeyi üç platformda çalışacak şekilde yeniden yazar.

Üç ayırt edici hedef:

1. **Davranışsal parite** — after-capture / after-upload görev zinciri, workflow sistemi, hotkey modeli, dosya adı token'ları birebir.
2. **Veri formatı uyumluluğu** — ShareX `.sxcu` (custom uploader) ve `.sxie` (image effect) dosyaları doğrudan import edilir. Gün 1'de yüzlerce hazır servis; rakiplerin hiçbirinde yok.
3. **Gerçek cross-platform** — tek kod tabanı, üç platformda ilk sınıf. Windows'ta ShareX'in yerini alabilecek, macOS'ta CleanShot X ile yarışabilecek, Linux'ta boş olan bir alanı dolduracak kalite.

---

## 1. Stack kararı

### 1.1 Değerlendirilen seçenekler

| Seçenek | Artı | Eksi | Karar |
|---|---|---|---|
| **Tauri v2** (Rust + web UI) | 5–15 MB binary, native sistem erişimi, mükemmel Rust capture ekosistemi, yerleşik tray/hotkey/updater, web tech ile üstün anotasyon canvas'ı | Rust öğrenme eğrisi, webview farklılıkları (WebKitGTK / WKWebView / WebView2) | ✅ **Seçildi** |
| Electron | En olgun ekosistem, tek webview (Chromium) | 150+ MB, yüksek RAM, ekran yakalama için yine native eklenti gerekir | ❌ |
| Avalonia (C#) | ShareX'in C# iş mantığı kısmen taşınabilir | Linux'ta cila zayıf, tasarım özgürlüğü sınırlı, ekosistem küçük | ❌ |
| Flutter Desktop | Tek render motoru, tutarlı görünüm | Linux'ta ekran yakalama eklentileri olgunlaşmamış, "native değil" hissi | ❌ |
| Rust + egui/iced | Tam native, tek dil | Zengin anotasyon editörü ve ayar arayüzleri için üretkenlik çok düşük | ❌ |

**Neden Tauri kazandı:** Bu uygulamanın iki zor kısmı var — (a) düşük seviye ekran/pencere/giriş erişimi, (b) zengin bir anotasyon editörü ve yoğun ayar arayüzleri. Rust (a)'da, web tech (b)'de en güçlü seçenek. Tauri ikisini tek binary'de birleştiriyor ve Electron'un boyut/bellek bedelini ödetmiyor.

### 1.2 Çekirdek bağımlılıklar

**Rust tarafı**

| Alan | Crate | Not |
|---|---|---|
| Uygulama kabuğu | `tauri` v2 | Pencere, tray, IPC, updater |
| Ekran yakalama | `xcap` | Win/Mac/Linux ekran + pencere yakalama, tek API |
| Yakalama (akış) | `scap` | ScreenCaptureKit / WGC / PipeWire üzerine akış — kayıt için |
| Global kısayol | `tauri-plugin-global-shortcut` | `global-hotkey` crate'i sarmalar |
| Pano | `arboard` | Görsel + metin, üç platform |
| Görüntü işleme | `image`, `imageproc`, `fast_image_resize` | Encode/decode, efektler |
| HTTP | `reqwest` (rustls) | Yükleme, OAuth |
| Async | `tokio` | |
| Serileştirme | `serde`, `serde_json` | Config, `.sxcu` |
| JSONPath | `serde_json_path` | Custom uploader `{json:}` |
| XPath | `sxd-xpath` veya `libxml` | Custom uploader `{xml:}` |
| Regex | `regex` | Custom uploader `{regex:}` |
| Veritabanı | `rusqlite` | Geçmiş |
| Sırlar | `keyring` | OS anahtar zinciri (Keychain / Credential Manager / Secret Service) |
| SFTP/SSH | `russh` + `russh-sftp` | |
| FTP | `suppaftp` | |
| S3 | `aws-sdk-s3` veya `rust-s3` | S3/R2/B2/Spaces/MinIO tek istemci |
| OCR | `ocrs` (birincil) + platform native (opsiyonel) | §4.13 |
| QR | `rqrr` (oku), `qrcode` (yaz) | |
| Hash | `sha2`, `md-5` | |
| Log | `tracing`, `tracing-subscriber` | |

**TypeScript tarafı**

| Alan | Paket |
|---|---|
| UI | React 19 |
| Build | Vite 7 |
| Durum | Zustand |
| Yönlendirme | TanStack Router |
| Stil | Tailwind CSS v4 + CSS değişkenleri |
| Bileşenler | Radix UI primitives (kendi stilimizle) |
| Canvas | Kendi yazdığımız katmanlı canvas motoru (bkz. §4.5) |
| Sanal liste | TanStack Virtual (kütüphane ızgarası) |
| Form | React Hook Form + Zod |
| i18n | `i18next` |

**Sidecar (isteğe bağlı, indirilerek kurulur)**

| Araç | Ne için | Zorunlu? |
|---|---|---|
| `ffmpeg` | Video kayıt encode, GIF, format dönüştürme | Kayıt için evet |

ShareX de ffmpeg'i bu şekilde bundle ediyor. Kestrel ffmpeg'i uygulama içinden indirir, sürümünü doğrular ve günceller; sistemde varsa onu kullanır.

---

## 2. Platform gerçekleri

Cross-platform vaadinin dürüst hâli. Her özellik her platformda aynı kalitede olmayacak; bu tabloyu kullanıcıya da göstereceğiz.

| Yetenek | macOS | Windows | Linux (X11) | Linux (Wayland) |
|---|---|---|---|---|
| Ekran/pencere yakalama | ScreenCaptureKit | Windows.Graphics.Capture / DXGI | XGetImage / XShm | PipeWire + xdg-desktop-portal |
| İzin gerekir mi | Ekran Kaydı (TCC) | Hayır | Hayır | Portal onayı (her oturum) |
| Global kısayol | Carbon hotkey | RegisterHotKey | XGrabKey | ⚠️ Kısıtlı — masaüstü ortamına bağlı |
| Şeffaf overlay penceresi | Evet | Evet | Evet (compositor gerekir) | ⚠️ Kısıtlı — konumlandırma sınırlı |
| Sistem tepsisi | NSStatusItem | Shell_NotifyIcon | libayatana-appindicator | libayatana-appindicator |
| Pencere listesi/bilgisi | ScreenCaptureKit + AX | EnumWindows | EWMH | ⚠️ Yok (protokol izin vermiyor) |
| Sistem sesi kaydı | ScreenCaptureKit audio | WASAPI loopback | PulseAudio monitor | PipeWire |
| Kaydırmalı yakalama | AX API + sentetik scroll | UI Automation + scroll | XTest + scroll | ⚠️ Çok kısıtlı |
| Native OCR | Vision | Windows.Media.Ocr | yok | yok |
| Otomatik güncelleme | Tauri updater | Tauri updater | AppImage/deb kanalı | aynı |

**Wayland stratejisi:** Wayland güvenlik modeli, bir uygulamanın diğer pencereleri görmesini ve global kısayol yakalamasını bilinçli olarak engelliyor. Kestrel'in yaklaşımı:
1. Yakalama için **xdg-desktop-portal** (ScreenCast) kullan — kullanıcı seçimi ile, protokole uygun.
2. Global kısayollar için **GlobalShortcuts portal**'ını dene; yoksa masaüstü ortamının kendi kısayol ayarına `kestrel capture region` komutunu bağlamayı öneren bir rehber göster.
3. Desteklenmeyen özellikleri arayüzde gri gösterip **nedenini açıkla** — sessizce kırılma yok.

---

## 3. Mimari

```
kestrel/
├── src/                        # React arayüz
│   ├── app/                    # Yönlendirme, kabuk, sağlayıcılar
│   ├── features/
│   │   ├── overlay/            # Bölge yakalama overlay UI
│   │   ├── editor/             # Anotasyon editörü
│   │   ├── library/            # Geçmiş / kütüphane
│   │   ├── workflows/          # Workflow düzenleyici
│   │   ├── destinations/       # Hedef yapılandırma + custom uploader
│   │   ├── tools/              # 24 yardımcı araç
│   │   └── settings/
│   ├── design/                 # Tasarım sistemi: token, primitives
│   └── lib/                    # IPC sarmalayıcıları, tipler (Rust'tan üretilir)
│
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── commands/           # IPC yüzeyi (frontend'in gördüğü tek API)
│   │   └── ...
│   └── tauri.conf.json
│
├── crates/                     # Rust workspace — çekirdek mantık
│   ├── kestrel-core/           # Domain: modeller, pipeline, workflow, token parser
│   ├── kestrel-capture/        # Ekran/pencere/bölge yakalama, platform soyutlaması
│   ├── kestrel-record/         # Video/GIF kayıt, ffmpeg orkestrasyonu
│   ├── kestrel-image/          # Efekt zinciri, encode/decode, .sxie
│   ├── kestrel-upload/         # Uploader trait, servisler, custom uploader motoru
│   ├── kestrel-tools/          # OCR, QR, hash, metadata, indexer...
│   └── kestrel-cli/            # `kestrel` komut satırı aracı
│
├── docs/
└── .github/workflows/          # CI: üç platformda build + test + release
```

**Kural:** `kestrel-core` hiçbir UI veya Tauri bağımlılığı içermez. CLI ve testler ondan bağımsız çalışır. Tauri katmanı yalnızca ince bir IPC kabuğudur.

**Tip güvenliği:** Rust tarafındaki tüm IPC tipleri `ts-rs` ile TypeScript tanımlarına derlenir. Frontend ile backend arasında elle senkronize edilen tip yok.

### 3.1 Veri akışı

```
Hotkey / Tray / CLI / URL şeması / Dosya yöneticisi
              │
              ▼
        WorkflowEngine        ← Workflow tanımı (yakalama yöntemi + görev ayarları)
              │
              ▼
        CaptureService  ──►  CaptureResult (görüntü / video / metin)
              │
              ▼
        TaskPipeline          ← AfterCaptureTasks (sıralı, iptal edilebilir)
              │  ├─ Güzelleştir → Efektler → Editör
              │  ├─ Kopyala / Kaydet / Küçük resim / Sabitle / Yazdır
              │  └─ OCR / QR / Analiz / Eylemler (harici program)
              ▼
        UploadService   ──►  UploadResult (URL, silme URL'si, thumb URL)
              │
              ▼
        TaskPipeline          ← AfterUploadTasks
              │  └─ Kısalt / Paylaş / Kopyala / Aç / QR
              ▼
        HistoryStore + Bildirim
```

ShareX'in akışının birebir aynısı — kullanıcı kas hafızası korunuyor.

---

## 4. Modül spec'leri

### 4.1 Yakalama (`kestrel-capture`)

Platform farklılıkları tek bir trait arkasında:

```rust
pub trait CaptureBackend: Send + Sync {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    fn windows(&self) -> Result<Vec<WindowInfo>>;
    fn capture_display(&self, id: DisplayId) -> Result<RgbaImage>;
    fn capture_window(&self, id: WindowId) -> Result<RgbaImage>;
    fn capture_region(&self, region: Region) -> Result<RgbaImage>;
    fn capabilities(&self) -> Capabilities;
}
```

`capabilities()` her platformda neyin desteklendiğini bildirir; arayüz buna göre kendini kısar.

**Yakalama yöntemleri (ShareX paritesi):** Tam ekran · Aktif pencere · Aktif ekran · Pencere menüsü · Ekran menüsü · Bölge · Bölge (sade) · Bölge (şeffaf) · Son bölge · Özel bölge · Ekran kaydı · Ekran kaydı (GIF) · Kaydırmalı yakalama · Otomatik yakalama

**Çoklu ekran ve DPI:** Her ekranın ölçek faktörü ayrı okunur. Karma DPI kurulumlarda mantıksal koordinatlar → fiziksel piksel dönüşümü tek bir yerde (`DisplayGeometry`) yapılır. Bu, cross-platform hataların en büyük kaynağı; merkezileştiriliyor.

### 4.2 Bölge yakalama overlay'i

Projenin kalbi. Her ekran için bir Tauri penceresi: `transparent: true`, `decorations: false`, `alwaysOnTop: true`, `fullscreen`, `skipTaskbar: true`.

**Kritik teknik:** Overlay açılmadan **önce** tüm ekranların donmuş görüntüsü alınır ve overlay'in arka planı olarak çizilir. Bu:
- İmleç titremesini ve canlı içerik kaymasını önler (ShareX ile aynı yaklaşım),
- Webview şeffaflığının platform bazlı tutarsızlığını tamamen bypass eder,
- Wayland'de zaten tek uygulanabilir yöntem.

**Etkileşimler (ShareX keybind tablosuyla birebir):**

| Girdi | Davranış |
|---|---|
| Sol tık sürükle / `Insert` | Bölge seçimi başlat/bitir |
| `Esc` / sağ tık | Kapat |
| `Space` | Tam ekran yakala |
| `1`–`0` | Belirli ekranı yakala |
| `~` | Aktif ekranı yakala |
| `Tab` / orta tık | Son bölge ↔ son çizim aracı geçişi |
| Ok tuşları | 1px oynat · `Shift`+Ok 10px |
| `Mod`+Ok | Sağ-alt köşeden yeniden boyutlandır |
| `Alt`+Ok | Sol-üst köşeden yeniden boyutlandır |
| Sürüklerken `Shift` | Orantılı |
| Sürüklerken `Alt` | Ön ayar boyutlara kilitle |
| Sürüklerken `Mod` | Şekli taşı |
| Tekerlek | Büyüteç boyutu |
| `Mod`+C | Konum & boyut bilgisini kopyala |
| `Mod`+Z / `Mod`+Shift+Z | Geri al / yinele |
| `Mod`+D | Şekli çoğalt |
| `Delete` / `Shift`+`Delete` | Şekli sil / hepsini sil |
| Home/End/PageUp/PageDown | Z-sırası |
| Çift tık / `Enter` | Çoklu bölge modunda yakala |

`Mod` = macOS'ta `Cmd`, Windows/Linux'ta `Ctrl`. Tüm kısayollar yeniden atanabilir.

**HUD:** Büyüteç (piksel ızgarası + hex kodu) · artı imleç · canlı boyut rozeti · pencereye yapışma · ön ayar oranlar · çoklu bölge · overlay içi renk seçici · 21 anotasyon aracı (editörle ortak motor) · alt araç çubuğu.

### 4.3 Kayıt (`kestrel-record`)

**Boru hattı:** `scap` ile platform akışı → ham kareler → `ffmpeg` (sidecar, stdin pipe) → mp4/webm/gif.

ShareX de ffmpeg kullanıyor; bu tercih pariteyi ve kaliteyi garantiler, üç platformda tek kod yolu verir.

- Codec: H.264 / HEVC / VP9 / AV1, donanım hızlandırma tespiti (`videotoolbox` / `nvenc` / `qsv` / `vaapi`)
- Ses: sistem sesi + mikrofon, ayrı veya karışık
- fps: 10/24/30/60 · alan: ekran/pencere/bölge
- Duraklat / devam / iptal · kayıt HUD'u · süre göstergesi
- İmleç göster/gizle, vurgula, tıklama efekti
- GIF: ffmpeg `palettegen`/`paletteuse` (ShareX ile aynı zincir)

### 4.4 Kaydırmalı yakalama

ShareX algoritması: ilk kare → sonraki kareleri öncekiyle karşılaştır → değişen kısmı ekle → sona gelene dek tekrarla. Yeşil/sarı/kırmızı güven göstergesi.

**Kestrel:**
1. Kullanıcı bölge seçer.
2. Sentetik kaydırma (`enigo` crate) veya platform otomasyon API'si.
3. Her adımda kare alınır.
4. Dikişleme: normalize edilmiş çapraz korelasyon (`imageproc`), SIMD hızlandırmalı.
5. Aynı yeşil/sarı/kırmızı gösterge.
6. **İyileştirme:** sabit header/footer otomatik tespit edilip bir kez dahil edilir (ShareX'te manuel).

Platform desteği: Windows 🟢 · macOS 🟡 (Erişilebilirlik izni) · X11 🟡 · Wayland 🔴

### 4.5 Anotasyon editörü

**Model:** Non-destructive vektörel katman listesi, tamamı serileştirilebilir. Sonuç: `.kestrel` dosyası sonradan tekrar düzenlenebilir (ShareX'te yok), undo/redo saf model diff'i, farklı çözünürlükte yeniden export mümkün.

**Render:** HTML Canvas 2D birincil; ağır efektler (bulanıklık, pikselleştirme, spot) için WebGL katmanı. Nihai export **Rust tarafında** yapılır — webview'in renk profili ve anti-aliasing farklılıkları çıktıyı platformlar arası tutarsız yapmasın diye. Yani canvas önizleme, Rust gerçek.

**20 araç (ShareX modern editörü birebir):** Seç `V` · Dikdörtgen `R` · Elips `E` · Çizgi `L` · Ok `A` · Serbest `F` · Metin `T` · Konuşma balonu `O` · Adım `N` · Görsel `I` · Emoji `J` · İmleç `K` · Vurgu `H` · Akıllı silgi `W` · Bulanıklaştır `B` · Pikselleştir `P` · Büyüteç `M` · Spot `S` · Kırp `C` · Kes `U`

**Eylemler:** `Enter` devam · `Esc` iptal · `Mod+Shift+C` kopyala · `Mod+S` kaydet · `Mod+Shift+S` farklı kaydet · `Mod+P` sabitle · `Mod+Shift+P` yazdır · `Mod+U` yükle · `Mod+Shift+F` düzleştir · z-sırası tuşları

**Arka plan / güzelleştirme:** margin · padding · akıllı padding · yuvarlak köşe · gölge · en-boy oranı · şeffaf/düz/gradyan/görsel/duvar kâğıdı arka plan · **3D perspektif eğimi** (yeni)

### 4.6 Efektler (`kestrel-image`)

Kategoriler: manipülasyonlar (boyutlandır, kırp, döndür, çevir, kanvas, otomatik kırp) · ayarlamalar (parlaklık, kontrast, gama, doygunluk, ton, alfa, renk matrisi) · filtreler (bulanıklık, keskinleştir, gürültü, piksel, gri, sepya, ters, dış hat, konvolüsyon) · çizimler (kenarlık, gölge, metin/görsel filigranı, arka plan, köşe kes).

Her efekt bir `ImageEffect` trait implementasyonu, zincir `Codable`. **`.sxie` JSON şemasıyla uyumlu okuyucu** yazılır; desteklenmeyen efektler uyarıyla atlanır.

### 4.7 Yükleme (`kestrel-upload`)

```rust
#[async_trait]
pub trait Uploader: Send + Sync {
    fn id(&self) -> &str;
    fn kinds(&self) -> DestinationKinds;   // image | text | file | url_shortener | url_sharing
    async fn upload(&self, payload: Payload, progress: ProgressSink) -> Result<UploadResult>;
}
```

**Kategoriler:** Görsel (Imgur, Chevereto, ImgBB, vgy.me…) · Metin (Pastebin, Gist, Hastebin, Paste.ee, GitLab snippet…) · Dosya (S3 uyumlu: AWS/R2/B2/Spaces/MinIO · Dropbox · Google Drive · OneDrive · Box · pCloud · FTP/FTPS/SFTP · GoFile · Uguu…) · URL kısaltıcı (bit.ly, is.gd, tinyurl, yourls, Kutt, Polr…) · URL paylaşım (Discord webhook, Slack, Telegram, Mastodon, Bluesky, e-posta…)

**Kimlik doğrulama:** OAuth2 + PKCE, yerel loopback callback sunucusu. Tüm token ve API anahtarları **OS anahtar zincirinde** (`keyring` crate) — config JSON'ında asla düz metin sır yok. ShareX'in `UploadersConfig.json`'ı düz metin tutar; bu bilinçli bir güvenlik iyileştirmesi.

**Altyapı:** Devam ettirilebilir yükleme, ilerleme raporlama, üstel geri çekilmeli yeniden deneme, eşzamanlılık limiti, hız sınırlama, çevrimdışı kuyruk.

### 4.8 Custom uploader motoru (`.sxcu` uyumlu) — en yüksek kaldıraçlı bileşen

ShareX'in tam syntax'ı desteklenir.

**Alanlar:** Name · DestinationType · RequestMethod (GET/POST/PUT/PATCH/DELETE) · RequestURL · Parameters · Headers · Body (None / MultipartFormData / FormURLEncoded / JSON / XML / Binary) · Arguments · FileFormName · URL · ThumbnailURL · DeletionURL · ErrorMessage

**13 syntax fonksiyonu, birebir:**

| Fonksiyon | Rust karşılığı |
|---|---|
| `{response}` | ham gövde |
| `{responseurl}` | yönlendirme sonrası URL |
| `{header:ad}` | yanıt başlığı |
| `{json:jsonPath}` | `serde_json_path` |
| `{xml:xpath}` | `sxd-xpath` |
| `{regex:desen\|grup}` | `regex` (indeks + isimli grup) |
| `{input}` | pipeline girdisi |
| `{filename}` | pipeline dosya adı |
| `{random:a\|b\|c}` | `rand` |
| `{select:a\|b\|c}` | seçim penceresi |
| `{inputbox:başlık\|varsayılan}` | girdi penceresi |
| `{outputbox:başlık\|metin}` | çıktı penceresi |
| `{base64:metin}` | `base64` |

**Kaçış kuralı:** `{`, `}`, `|`, `\` karakterleri `\` ile kaçırılır — birebir.

**Kestrel ekleri:** `.sxcu` dosya ilişkilendirme ve sürükle-bırak import · yerleşik istek/yanıt inspector'ı (ShareX'te yok) · [ShareX/CustomUploaders](https://github.com/ShareX/CustomUploaders) deposunu uygulama içinden taranabilir katalog olarak sunma.

### 4.9 Dosya adı token sistemi

ShareX'in `%` token'ları birebir:

**Pencere:** `%t` `%pn`
**Tarih/saat:** `%y` `%yy` `%mo` `%mon` `%mon2` `%w` `%w2` `%wy` `%d` `%h` `%mi` `%s` `%ms` `%pm` `%unix`
**Artan:** `%i` `%ia` `%iAa` `%ib` `%ix` (`{n}` ile sıfır dolgusu)
**Rastgele:** `%rn` `%ra` `%rna` `%rx` `%guid` `%radjective` `%ranimal` `%remoji` `%rf{yol}`
**Görsel:** `%width` `%height`
**Bilgisayar:** `%un` `%uln` `%cn`
**Diğer:** `%n`

**Ekler:** `%app` (yakalanan uygulama adı) · `%ocr` (tanınan ilk satır)

Arayüzde kategorili token menüsü + **canlı önizleme** (ShareX'te yok).

### 4.10 Workflow & hotkey motoru

**Workflow = ad + kısayol + yakalama yöntemi + görev ayarı geçersiz kılmaları.**

ShareX'in en güçlü fikri. Her workflow bağımsız `TaskSettings` snapshot'ı taşır; varsayılan ayarlardan miras alır, üzerine yazar. Workflow'lar JSON olarak dışa/içe aktarılabilir.

**After-capture görevleri (22, birebir sıralı):** Hızlı görev menüsü · "Yakalama sonrası" penceresi · Güzelleştir · Efekt uygula · Editörde aç · Panoya kopyala · Ekrana sabitle · Yazdır · Dosyaya kaydet · Farklı kaydet · Küçük resim kaydet · Eylemleri çalıştır · Dosyayı panoya kopyala · Dosya yolunu kopyala · Klasör yolunu kopyala · Dosya yöneticisinde göster · Analiz et · QR tara · OCR · "Yükleme öncesi" penceresi · Yükle · Yerelden sil

**After-upload görevleri (6, birebir):** "Yükleme sonrası" penceresi · URL kısalt · URL paylaş · URL kopyala · URL aç · QR göster

### 4.11 Eylemler (harici programlar)

ShareX'in `$input` / `$output` değişken sistemi birebir. Hazır ön ayarlar platforma göre uyarlanır (pngquant, oxipng, cwebp, cjpeg, ffmpeg zincirleri). Kestrel eksik araçları tespit edip platforma uygun kurulum komutunu önerir (`brew` / `winget` / `apt`).

### 4.12 Araçlar (24, ShareX paritesi)

Renk seçici · Ekran renk seçici · Cetvel · Ekrana sabitle · Görsel editörü · Güzelleştirici · Efektler · Görüntüleyici · Arka plan kaldırıcı · Karşılaştırıcı · Birleştirici · Bölücü · Küçük resim üretici · Video dönüştürücü · Video küçük resmi · Görsel analizi · OCR · QR kod · Hash kontrolü · Metadata (görüntüle + temizle) · Dizin indeksleyici · Pano görüntüleyici · Kenarlıksız pencere · Pencere inceleme · Monitör testi

### 4.13 OCR

Cross-platform OCR'ın dürüst tablosu:

| Yaklaşım | Kalite | Boyut | Platform |
|---|---|---|---|
| **`ocrs`** (Rust, ML tabanlı) | İyi | ~15 MB model | Üçü de |
| Platform native (Vision / Windows.Media.Ocr) | Çok iyi | 0 | macOS, Windows |
| Tesseract sidecar | Orta-iyi | ~50 MB + dil paketleri | Üçü de |

**Karar:** Varsayılan `ocrs` (tek kod yolu, makul kalite, dil kurulumu yok). macOS ve Windows'ta native motor **opsiyonel yüksek kalite modu** olarak sunulur — kullanıcı ayarlardan seçer. Linux'ta `ocrs` tek seçenek.

Arayüz: görselin üstünde seçilebilir metin katmanı, satır/blok kopyalama, tam metin paneli.

### 4.14 Geçmiş & kütüphane

SQLite (`rusqlite`). ShareX'in `History.xml` dosyası **import edilebilir** — Windows'tan göç yolu.

Izgara/liste görünümü · tarih bölümleme · filtre çipleri (tür, hedef, tarih) · **tam metin arama (OCR metni dahil)** · toplu işlemler · sürükle-bırak dışa aktarma · izleme klasörü (`notify` crate ile FS izleme → otomatik yükleme).

### 4.15 Entegrasyonlar

| Yüzey | Uygulama |
|---|---|
| Sistem tepsisi | Tauri tray + hızlı erişim paneli |
| Global kısayollar | `tauri-plugin-global-shortcut`, workflow başına |
| CLI | `kestrel capture region`, `kestrel upload dosya.png --workflow "Imgur"` |
| URL şeması | `kestrel://capture/region`, `kestrel://workflow/<ad>` |
| Dosya yöneticisi | macOS Quick Action · Windows shell verb · Linux `.desktop` action |
| Sürükle-bırak | Tray ikonuna / kütüphane penceresine dosya bırakıp yükleme |
| Bildirimler | Native bildirim + tıklama eylemleri |
| Otomatik güncelleme | Tauri updater, imzalı manifest |

---

## 5. Veri düzeni

```
<config>/kestrel/
├── settings.json
├── workflows.json
├── destinations.json        # SIRSIZ — sırlar OS anahtar zincirinde
├── uploaders/               # .ksu / .sxcu
├── effects/                 # .ksie / .sxie
├── history.sqlite
└── logs/

<pictures>/Kestrel/2026/08/  # varsayılan çıktı

Anahtar zinciri: kestrel.<servis>
```

`<config>` = macOS `~/Library/Application Support` · Windows `%APPDATA%` · Linux `$XDG_CONFIG_HOME`

**Taşınabilir mod:** Binary yanında `portable` dosyası varsa tüm veri uygulama klasörüne yazılır (ShareX davranışı birebir).

---

## 6. Test stratejisi

| Katman | Yaklaşım |
|---|---|
| `kestrel-core` | Rust birim testleri — token parser, workflow motoru, pipeline sırası, `.sxcu` parser (ShareX/CustomUploaders deposundaki gerçek dosyalarla) |
| Uploader'lar | `wiremock` ile kayıtlı istek/yanıt fixtures |
| Yakalama | Her platformda CI smoke testi + altın görüntü karşılaştırması |
| Görüntü işleme | Piksel toleranslı snapshot testleri |
| Frontend | Vitest + Testing Library; canvas motoru için birim testler |
| E2E | WebDriver (`tauri-driver`) ile kritik akışlar |
| Erişilebilirlik | axe-core CI kontrolü + manuel ekran okuyucu geçişi |

**CI:** GitHub Actions matrix — `macos-latest` (arm64 + x86_64), `windows-latest`, `ubuntu-latest`. Her PR'da build + test + lint (`clippy`, `rustfmt`, `eslint`, `tsc`).

---

## 7. Yol haritası

Efor tahminleri tek geliştirici / tam zamanlı hafta.

### Faz 0 — Temel (2 hafta)
Tauri iskeleti · Rust workspace · tasarım sistemi · ayar/persistence katmanı · anahtar zinciri · loglama · tray · CI (üç platform) · updater iskeleti
**Çıktı:** Üç platformda derlenen, güncellenebilir boş kabuk

### Faz 1 — Yakalama çekirdeği (3 hafta)
Platform capture backend'leri · bölge overlay'i (çoklu ekran, karma DPI, büyüteç, yapışma) · son/özel/çoklu bölge · global hotkey motoru · temel after-capture (kopyala/kaydet/göster)
**Çıktı:** Günlük kullanılabilir ekran görüntüsü aracı

### Faz 2 — Editör (4 hafta)
Vektörel anotasyon modeli + undo/redo · 20 araç · canvas motoru + Rust export · arka plan/güzelleştirme · ekrana sabitleme · yakalama sonrası yüzen kart
**Çıktı:** CleanShot X ile yarışabilir ürün

### Faz 3 — Yükleme & workflow (4 hafta)
Uploader trait + pipeline · **custom uploader motoru + `.sxcu` import** (öncelikli) · ilk dalga servisler · OAuth altyapısı · workflow motoru + görsel düzenleyici · token sistemi · geçmiş + kütüphane
**Çıktı:** ShareX'in ana değer önerisi tamam

### Faz 4 — Kayıt (3 hafta)
ffmpeg sidecar yönetimi · `scap` akış boru hattı · ses · duraklat/devam · GIF · kayıt HUD'u · video araçları
**Çıktı:** Ekran kaydı paritesi

### Faz 5 — Araçlar & efektler (3 hafta)
24 aracın kalanı · efekt zinciri + `.sxie` import · OCR · QR · hash · metadata · indexer
**Çıktı:** Araç kutusu paritesi

### Faz 6 — Entegrasyon & cila (3 hafta)
CLI · URL şeması · dosya yöneticisi entegrasyonu · izleme klasörü · kaydırmalı yakalama (en riskli, bilinçli olarak sonda) · erişilebilirlik · lokalizasyon (TR/EN) · performans · dağıtım kanalları (Homebrew, winget, AUR, Flathub) · web sitesi
**Çıktı:** 1.0

**Toplam ~22 hafta.** İlk kullanılabilir sürüm (Faz 0–2) **~9 hafta**.

---

## 8. Riskler

| Risk | Etki | Azaltma |
|---|---|---|
| Wayland kısıtları | Yüksek | Portal tabanlı yol + yeteneklerin şeffaf raporlanması; X11'de tam destek |
| Kaydırmalı yakalama güvenilirliği | Orta | Platform bazlı strateji, "beta" etiketi, açık beklenti yönetimi |
| Webview farklılıkları (WebKitGTK/WebView2/WKWebView) | Orta | Nihai render Rust'ta; webview sadece önizleme. CI'da üç platformda görsel test |
| ffmpeg bağımlılığı | Orta | Uygulama içi indirme + sürüm doğrulama; sistem kurulumu varsa onu kullan |
| 80+ uploader bakım yükü | Yüksek | Custom uploader birinci sınıf; native liste yaşayan servislerle sınırlı; topluluk şablonu |
| Rust öğrenme eğrisi | Orta | Çekirdek mantık Rust, arayüz TS — iş yükünün çoğu tanıdık tarafta |
| Kod imzalama maliyeti (mac + Windows) | Düşük | 1.0'a kadar imzasız + net kurulum rehberi; sonra sponsorluk/bağış ile |
| Tek geliştirici kapsamı | Yüksek | Fazlar bağımsız değer üretir; Faz 2 sonunda yayınlanabilir ürün |

---

## 9. Yönetişim

- **Lisans:** GPL-3.0 — ShareX ile aynı ruh, kapalı ticari klonları engeller.
- **Sürümleme:** SemVer, `main` sürekli yayınlanabilir.
- **Katkı:** Uploader eklemek için tek dosyalık şablon + test fixture kuralı.
- **Dağıtım:** GitHub Releases (dmg/msi/AppImage/deb/rpm) + Homebrew Cask + winget + AUR + Flathub.
