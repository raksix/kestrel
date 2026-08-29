# Kestrel — ShareX özellik dökümü ve backlog

Bu doküman ShareX'in **her** belgelenmiş özelliğini madde madde listeler ve
Kestrel'deki karşılığını, durumunu, hangi fazda geleceğini gösterir. Projenin
ana backlog'u budur — `01-FEATURE-PARITY.md` özet, bu ise tam liste.

**Durum işaretleri**
- `[x]` — yapıldı
- `[~]` — kısmen yapıldı
- `[ ]` — planlandı, henüz yapılmadı
- `[-]` — kapsam dışı (gerekçesiyle)

**Kaynak:** ShareX 17.x dokümantasyonu (getsharex.com), `Enums.cs`,
`CodeMenuEntryFilename.cs`, `_data/actions.yml` ve uygulama içi menüler.

---

## 1. Yakalama yöntemleri (14)

| # | Özellik | Durum | Faz | Not |
|---|---|:-:|:-:|---|
| 1.1 | Fullscreen — tüm ekranlar tek görüntü | `[x]` | 1 | Karma DPI'da fiziksel piksel kompozitleme |
| 1.2 | Active window — öndeki pencere | `[x]` | 1 | Odaklı pencere, yoksa en üstteki |
| 1.3 | Active monitor — imlecin ekranı | `[x]` | 1 | |
| 1.4 | Window menu — pencere listesinden seç | `[x]` | 1 | Küçük resimli seçici penceresi |
| 1.5 | Monitor menu — ekran listesinden seç | `[x]` | 1 | Aynı seçicinin "Ekranlar" sekmesi |
| 1.6 | Region — bölge seçimi | `[x]` | 1 | Donmuş kare + overlay |
| 1.7 | Region (Light) — sade tema | `[ ]` | 2 | Overlay teması varyantı |
| 1.8 | Region (Transparent) — karartmasız | `[ ]` | 2 | Overlay teması varyantı |
| 1.9 | Last region — son bölgeyi tekrar kullan | `[ ]` | 2 | Ekran kimliğiyle birlikte kalıcı |
| 1.10 | Custom region — kayıtlı sabit bölge | `[ ]` | 2 | Adlandırılmış bölge listesi |
| 1.11 | Screen recording — video | `[ ]` | 4 | ffmpeg sidecar |
| 1.12 | Screen recording (GIF) | `[ ]` | 4 | palettegen/paletteuse zinciri |
| 1.13 | Scrolling capture — kaydırmalı | `[ ]` | 6 | Platform otomasyonuna bağlı |
| 1.14 | Auto capture — zamanlayıcılı | `[ ]` | 2 | Aralık + sabit bölge |

**Kestrel eki:** gecikmeli yakalama (3/5/10 sn geri sayım), her ekranı ayrı
dosyaya kaydetme.

---

## 2. Bölge yakalama overlay'i

### 2.1 Seçim
| Özellik | Durum | Faz |
|---|:-:|:-:|
| Dikdörtgen bölge | `[x]` | 1 |
| Elips bölge | `[ ]` | 2 |
| Serbest çizim bölge | `[ ]` | 2 |
| Çoklu bölge modu (`Enter`/çift tık ile bitir) | `[ ]` | 2 |
| Pencereye yapışma (hover → vurgula, tıkla → yakala) | `[x]` | 1 |
| Ön ayar oranlara kilitleme (`Alt`) | `[ ]` | 2 |
| Orantılı yeniden boyutlandırma (`Shift`) | `[x]` | 1 |
| Tutamaklarla yeniden boyutlandırma | `[~]` | 2 | görsel var, sürükleme faz 2 |
| Seçimi taşıma | `[ ]` | 2 |

### 2.2 HUD
| Özellik | Durum | Faz |
|---|:-:|:-:|
| Canlı boyut göstergesi | `[x]` | 1 |
| Artı imleç (crosshair) | `[x]` | 1 |
| İpucu şeridi | `[x]` | 1 |
| Büyüteç + piksel ızgarası + hex kodu | `[ ]` | 2 |
| Overlay içi renk seçici | `[ ]` | 2 |
| Alt araç çubuğu (anotasyon araçları) | `[ ]` | 2 |

### 2.3 Klavye (ShareX keybind tablosu)
| Tuş | Davranış | Durum |
|---|---|:-:|
| Sol tık sürükle | Bölge seç | `[x]` |
| `Esc` / sağ tık | İptal | `[x]` |
| `Space` | Tam ekran yakala | `[x]` |
| `Enter` | Seçimi onayla | `[x]` |
| Ok tuşları | 1px oynat | `[x]` |
| `Shift`+Ok | 10px oynat | `[x]` |
| `Alt`+Ok | Yeniden boyutlandır | `[x]` |
| `1`–`0` | Belirli ekranı yakala | `[ ]` |
| `~` | Aktif ekranı yakala | `[ ]` |
| `Tab` | Son bölge ↔ son araç geçişi | `[ ]` |
| Orta tık | Aynı geçiş | `[ ]` |
| Fare tekerleği | Büyüteç boyutu | `[ ]` |
| `Mod`+C | Konum & boyut kopyala | `[ ]` |
| `Mod`+Z / `Mod`+Shift+Z | Geri al / yinele | `[ ]` |
| `Mod`+D | Şekli çoğalt | `[ ]` |
| `Delete` / `Shift`+`Delete` | Şekli / hepsini sil | `[ ]` |
| Home/End/PageUp/PageDown | Z-sırası | `[ ]` |

---

## 3. Görsel editörü

### 3.1 Anotasyon araçları (20)
Tümü faz 2. Kısayollar ShareX modern editörüyle birebir.

| Kısayol | Araç | Durum | Not |
|---|---|:-:|---|
| `V` | Seç / taşı | `[x]` | Tıkla seç, sürükle taşı |
| `R` | Dikdörtgen | `[x]` | kenarlık, dolgu, kalınlık, köşe yarıçapı |
| `E` | Elips | `[x]` | |
| `L` | Çizgi | `[x]` | `Shift` 45° kilit faz 2'nin kalanında |
| `A` | Ok | `[x]` | uç stilleri modelde var |
| `F` | Serbest çizim | `[x]` | |
| `N` | Adım numarası | `[x]` | otomatik artan, silmede yeniden numaralanır |
| `H` | Vurgu | `[x]` | |
| `B` | Bulanıklaştır | `[x]` | örnekleme alana sınırlı |
| `P` | Pikselleştir | `[x]` | |
| `S` | Spot ışığı | `[x]` | |
| `T` | Metin | `[ ]` | font altyapısı bekliyor |
| `O` | Konuşma balonu | `[ ]` | font altyapısı bekliyor |
| `I` | Görsel ekle | `[ ]` | |
| `J` | Emoji | `[ ]` | |
| `K` | İmleç | `[ ]` | |
| `W` | Akıllı silgi | `[ ]` | |
| `M` | Büyüteç | `[ ]` | |
| `C` | Kırp | `[ ]` | |
| `U` | Kes (cut out) | `[ ]` | |

### 3.2 Editör eylemleri
| Kısayol | Eylem | Durum |
|---|---|:-:|
| `Enter` | Kaydet ve göreve devam et | `[~]` |
| `Esc` | İptal | `[x]` |
| `Mod+S` / `Mod+Shift+S` | Kaydet / farklı kaydet | `[ ]` |
| `Mod+Shift+C` | Panoya kopyala | `[ ]` |
| `Mod+U` | Yükle | `[ ]` |
| `Mod+P` | Ekrana sabitle | `[ ]` |
| `Mod+Shift+P` | Yazdır | `[ ]` |
| `Mod+Shift+F` | Anotasyonları düzleştir | `[ ]` |
| `Mod+Z` / `Mod+Shift+Z` | Geri al / yinele | `[x]` |
| Orta tık sürükle | Görüntüyü kaydır | `[ ]` |
| `Mod`+tekerlek | İmleç merkezli zoom | `[ ]` |
| `Mod+0` / `Mod+Alt+0` | Zoom sıfırla / sığdır | `[ ]` |
| Home/End/PageUp/PageDown | Z-sırası | `[ ]` |

### 3.3 Arka plan ve güzelleştirme (Image Beautifier)
Kenar boşluğu · iç boşluk · akıllı padding · yuvarlatılmış köşe · gölge ·
en-boy oranı · arka plan (şeffaf / düz renk / gradyan / görsel / duvar kâğıdı)
— tümü `[ ]` faz 2.

**Kestrel eki:** 3B perspektif eğimi.

### 3.4 Editör seçenekleri
Tema · sistem vurgu rengini takip · pencere durumunu hatırla · çıkış onayı ·
açılışta sığdır · hızlı kırpma · görev bitince otomatik kapat · panoya otomatik
kopyala · görsel ekleme diyaloğunu göster — tümü `[ ]` faz 2.

---

## 4. Görsel efektleri (`.sxie` uyumlu)

Tümü faz 5.

**Manipülasyonlar:** yeniden boyutlandır · kırp · otomatik kırp · kanvas ·
döndür · çevir · yuvarlatılmış köşe · kenar yumuşatma · şeffaflık zemini

**Ayarlamalar:** parlaklık · kontrast · gama · doygunluk · ton · alfa ·
renk matrisi · seviyeler · eğriler · renk dengesi

**Filtreler:** bulanıklık · gauss bulanıklığı · keskinleştir · gürültü ekle ·
pikselleştir · gri ton · sepya · negatif · dış hat (edge detect) · emboss ·
konvolüsyon matrisi · renk ayırma (posterize) · eşikleme

**Çizimler:** kenarlık · gölge · metin filigranı · görsel filigranı ·
arka plan · parçacık · köşe kesme · çerçeve

**Altyapı:** efekt zinciri sırası · ön ayar kaydet/yükle · `.sxie` içe aktarma ·
canlı önizleme · öncesi/sonrası karşılaştırma.

---

## 5. After-capture görevleri (22)

Pipeline bu sırayla çalışır (ShareX ile birebir).

| # | Görev | Durum | Faz |
|---|---|:-:|:-:|
| 1 | Hızlı görev menüsünü göster | `[ ]` | 3 |
| 2 | "Yakalama sonrası" penceresini göster | `[ ]` | 3 |
| 3 | Görseli güzelleştir | `[ ]` | 2 |
| 4 | Görsel efektleri uygula | `[ ]` | 5 |
| 5 | Editörde aç | `[x]` | 2 |
| 6 | Panoya kopyala | `[x]` | 1 |
| 7 | Ekrana sabitle | `[ ]` | 2 |
| 8 | Yazdır | `[ ]` | 3 |
| 9 | Dosyaya kaydet | `[x]` | 1 |
| 10 | Farklı kaydet | `[ ]` | 2 |
| 11 | Küçük resmi dosyaya kaydet | `[ ]` | 3 |
| 12 | Eylemleri çalıştır (harici program) | `[ ]` | 5 |
| 13 | Dosyayı panoya kopyala | `[ ]` | 2 |
| 14 | Dosya yolunu panoya kopyala | `[ ]` | 2 |
| 15 | Klasör yolunu panoya kopyala | `[ ]` | 2 |
| 16 | Dosya yöneticisinde göster | `[ ]` | 2 |
| 17 | Görseli analiz et | `[ ]` | 5 |
| 18 | QR kodu tara | `[ ]` | 5 |
| 19 | Metin tanı (OCR) | `[ ]` | 5 |
| 20 | "Yükleme öncesi" penceresini göster | `[ ]` | 3 |
| 21 | Görseli yükle | `[ ]` | 3 |
| 22 | Dosyayı yerelden sil | `[ ]` | 3 |

**Kestrel eki:** AirDrop / sistem paylaşımı, betik çalıştırma.

---

## 6. After-upload görevleri (6)

| # | Görev | Durum | Faz |
|---|---|:-:|:-:|
| 1 | "Yükleme sonrası" penceresini göster | `[ ]` | 3 |
| 2 | URL kısalt | `[ ]` | 4 |
| 3 | URL paylaş | `[ ]` | 4 |
| 4 | URL'yi panoya kopyala | `[ ]` | 3 |
| 5 | URL'yi aç | `[ ]` | 3 |
| 6 | QR kod penceresini göster | `[ ]` | 5 |

---

## 7. Yükleme yöntemleri (8)

| Yöntem | Durum | Faz |
|---|:-:|:-:|
| Dosya yükle | `[ ]` | 3 |
| Klasör yükle (opsiyonel arşivleme) | `[ ]` | 4 |
| Panodan yükle | `[ ]` | 3 |
| Metin yükle | `[ ]` | 3 |
| URL'den yükle (indir → yükle) | `[ ]` | 4 |
| Sürükle-bırak yükleme | `[ ]` | 3 |
| URL kısalt | `[ ]` | 4 |
| İzleme klasörü (watch folder) | `[ ]` | 6 |

---

## 8. Hedefler (destinations)

### 8.1 Custom uploader — en yüksek öncelik
`.sxcu` formatı birebir. Faz 3. Bu tek başına yüzlerce servisi açar.

**Alanlar:** Name · DestinationType · RequestMethod · RequestURL · Parameters ·
Headers · Body · Arguments · FileFormName · URL · ThumbnailURL · DeletionURL ·
ErrorMessage

**Body tipleri:** None · MultipartFormData · FormURLEncoded · JSON · XML · Binary

**HTTP metodları:** GET · POST · PUT · PATCH · DELETE

**Syntax fonksiyonları (13):**

| Fonksiyon | Açıklama |
|---|---|
| `{response}` | Ham yanıt gövdesi |
| `{responseurl}` | Yönlendirme sonrası URL |
| `{header:ad}` | Yanıt başlığı değeri |
| `{json:jsonPath}` | JSONPath ile ayrıştırma |
| `{xml:xpath}` | XPath ile ayrıştırma |
| `{regex:desen}` `{regex:desen\|grup}` | Regex, indeks veya isimli grup |
| `{input}` | Yüklenen metin / URL |
| `{filename}` | Dosya adı |
| `{random:a\|b\|c}` | Rastgele seçim |
| `{select:a\|b\|c}` | Kullanıcıya seçim penceresi |
| `{inputbox:başlık\|varsayılan}` | Kullanıcıdan metin iste |
| `{outputbox:başlık\|metin}` | Sonucu göster |
| `{base64:metin}` | Base64 kodlama |

**Kaçış:** `{`, `}`, `|`, `\` karakterleri `\` ile kaçırılır.

**Kestrel ekleri:** istek/yanıt inspector'ı · `.sxcu` sürükle-bırak import ·
topluluk uploader kataloğunu uygulama içinden tarama.

### 8.2 Görsel hedefleri
Imgur · ImageShack · Flickr · Chevereto · vgy.me · ImgBB · Lithiio · ImgPile ·
ImageBin · SomeImage · Photobucket · Twitter/X · Custom

### 8.3 Metin hedefleri
Pastebin · Paste.ee · Gist · GitLab Snippets · Hastebin · Pastie · OneTimeSecret ·
Custom

### 8.4 Dosya hedefleri
Dropbox · Google Drive · OneDrive · Box · Mega · pCloud · Amazon S3 ·
Cloudflare R2 · Backblaze B2 · DigitalOcean Spaces · MinIO · Google Cloud Storage ·
Azure Storage · FTP · FTPS · SFTP · SMB paylaşımı · GoFile · Uguu · Lambda ·
Teknik.io · file.io · put.re · Streamable · YouTube · Gfycat · Custom

### 8.5 URL kısaltıcılar
bit.ly · is.gd · v.gd · tinyurl · yourls · Polr · Kutt · Firebase Dynamic Links ·
adf.ly · Custom

### 8.6 URL paylaşım servisleri
Twitter/X · Mastodon · Bluesky · Reddit · Facebook · LinkedIn · Discord webhook ·
Slack · Telegram · WhatsApp · E-posta · Pushbullet · Pinterest · Tumblr ·
Google+ (ölü) · VK

### 8.7 Kimlik doğrulama altyapısı
OAuth 2.0 + PKCE · yerel loopback callback · API anahtarı · HTTP Basic ·
token yenileme · **tüm sırlar OS anahtar zincirinde** (ShareX düz metin JSON'da
tutar; bu bilinçli bir güvenlik iyileştirmesi).

### 8.8 Yükleme altyapısı
İlerleme raporu · iptal · yeniden deneme (üstel geri çekilme) · eşzamanlılık
limiti · hız sınırı · çevrimdışı kuyruk · devam ettirilebilir yükleme ·
yükleme geçmişi · silme URL'si saklama.

---

## 9. Dosya adı token'ları (37) — `[x]` faz 1, tamamlandı

**Pencere**
`%t` pencere başlığı · `%pn` süreç adı

**Tarih ve saat**
`%y` yıl · `%yy` yıl (2 hane) · `%mo` ay · `%mon` ay adı (yerel) ·
`%mon2` ay adı (İngilizce) · `%w` gün adı (yerel) · `%w2` gün adı (İngilizce) ·
`%wy` yılın haftası · `%d` gün · `%h` saat · `%mi` dakika · `%s` saniye ·
`%ms` milisaniye · `%pm` AM/PM · `%unix` unix zaman damgası

**Artan sayaç** (hepsi `{n}` ile sola sıfır dolgusu)
`%i` ondalık · `%ia` alfanümerik (harf duyarsız, taban 36) ·
`%iAa` alfanümerik (harf duyarlı, taban 62) · `%ib` `{n}` tabanında ·
`%ix` onaltılık

**Rastgele** (hepsi `{n}` ile tekrar)
`%rn` rakam · `%ra` alfanümerik · `%rna` karışmayan alfanümerik ·
`%rx` onaltılık · `%guid` GUID · `%radjective` sıfat · `%ranimal` hayvan ·
`%remoji` emoji · `%rf{dosyayolu}` dosyadan rastgele satır

**Görsel**
`%width` genişlik · `%height` yükseklik

**Bilgisayar**
`%un` kullanıcı adı · `%uln` oturum adı · `%cn` bilgisayar adı

**Diğer**
`%n` yeni satır

**Kestrel ekleri:** `%app` yakalanan uygulama adı · `%ocr` tanınan ilk satır ·
kategorili token menüsü · **canlı önizleme** (ShareX'te yok).

---

## 10. Araçlar (24)

| # | Araç | Durum | Faz | Not |
|---|---|:-:|:-:|---|
| 1 | Renk seçici | `[ ]` | 5 | Hex/RGB/HSL/HSV, palet geçmişi |
| 2 | Ekran renk seçici | `[ ]` | 2 | Overlay + büyüteç |
| 3 | Cetvel | `[ ]` | 5 | Mesafe ve açı ölçümü |
| 4 | Ekrana sabitle (Pin to screen) | `[ ]` | 2 | Ölçek, opaklık, gölge, kenarlık |
| 5 | Görsel editörü | `[ ]` | 2 | |
| 6 | Görsel güzelleştirici | `[ ]` | 2 | |
| 7 | Görsel efektleri | `[ ]` | 5 | |
| 8 | Görsel görüntüleyici | `[ ]` | 5 | |
| 9 | Arka plan kaldırıcı | `[ ]` | 5 | ONNX modeli, uygulama içi indirme |
| 10 | Görsel karşılaştırıcı | `[ ]` | 5 | Yan yana / geçiş / fark |
| 11 | Görsel birleştirici | `[ ]` | 5 | Dikey / yatay / ızgara |
| 12 | Görsel bölücü | `[ ]` | 5 | Izgaraya böl |
| 13 | Küçük resim üretici | `[ ]` | 5 | Toplu |
| 14 | Video dönüştürücü | `[ ]` | 4 | ffmpeg ön ayarları |
| 15 | Video küçük resmi | `[ ]` | 4 | Zaman damgalı ızgara |
| 16 | Görsel analizi | `[ ]` | 5 | Boyut, format, histogram, baskın renkler, EXIF |
| 17 | OCR | `[ ]` | 5 | `ocrs` + opsiyonel native motor |
| 18 | QR kod (oluştur + tara) | `[ ]` | 5 | |
| 19 | Hash kontrolü | `[ ]` | 5 | MD5/SHA1/SHA256/SHA512 |
| 20 | Metadata | `[ ]` | 5 | EXIF/IPTC/XMP görüntüle + temizle |
| 21 | Dizin indeksleyici | `[ ]` | 5 | HTML/TXT/XML/JSON |
| 22 | Pano görüntüleyici | `[ ]` | 5 | |
| 23 | Kenarlıksız pencere | `[ ]` | 6 | Platform otomasyonu |
| 24 | Pencere inceleme | `[ ]` | 6 | Hiyerarşi, frame, PID |
| 25 | Monitör testi | `[ ]` | 5 | Ölü piksel / renk testi |

---

## 11. Eylemler (harici programlar)

**Değişkenler:** `$input` giriş dosyası yolu · `$output` çıkış dosyası yolu

**Hazır ön ayarlar** (platforma göre uyarlanmış):

| Ön ayar | Komut | Çıktı |
|---|---|---|
| PNG sıkıştır (kayıplı) | `pngquant --ext .png --force --skip-if-larger --speed 3 --strip "$input"` | — |
| PNG sıkıştır (kayıpsız) | `oxipng -o 4 --strip safe "$input"` | — |
| WebP'ye çevir | `cwebp "$input" -q 80 -o "$output"` | webp |
| JPEG'e çevir (MozJPEG) | `cjpeg -quality 88 -outfile "$output" "$input"` | jpg |
| JPEG'e çevir (ImageMagick) | `magick "$input" -quality 85 -sampling-factor 4:2:0 -strip "$output"` | jpg |
| Video → H.264 | `ffmpeg -i "$input" -c:v libx264 -crf 23 -preset medium -movflags +faststart -c:a aac -b:a 128k -y "$output"` | mp4 |
| Video → HEVC | `ffmpeg -i "$input" -c:v libx265 -crf 28 -tag:v hvc1 -c:a aac -y "$output"` | mp4 |
| Video → VP9 | `ffmpeg -i "$input" -c:v libvpx-vp9 -crf 31 -b:v 0 -c:a libopus -y "$output"` | webm |
| Video → AV1 | `ffmpeg -i "$input" -c:v libsvtav1 -crf 35 -c:a libopus -y "$output"` | mkv |
| Video → GIF | `ffmpeg -i "$input" -lavfi "palettegen=stats_mode=full[p],[0:v][p]paletteuse=dither=sierra2_4a" -y "$output"` | gif |
| ZIP'e sıkıştır | platforma göre `ditto` / `7z` / `zip` | zip |

**Kestrel eki:** eksik aracı tespit edip platforma uygun kurulum komutunu öner
(`brew` / `winget` / `apt`). Faz 5.

---

## 12. Workflow ve otomasyon

| Özellik | Durum | Faz |
|---|:-:|:-:|
| Workflow = ad + kısayol + yöntem + görev ayarları | `[x]` | 1 |
| Global kısayol kaydı | `[x]` | 1 |
| **Kısayol düzenleme arayüzü** | `[x]` | 1 |
| Kısayol çakışma tespiti (uygulama içi) | `[x]` | 1 |
| OS'un kısayolu reddettiğini bildirme | `[x]` | 1 |
| Workflow başına ayar geçersiz kılma | `[~]` | 3 |
| Workflow'u etkinleştir/devre dışı bırak | `[x]` | 1 |
| Kısayolları toptan devre dışı bırak | `[ ]` | 3 |
| Workflow düzenleyici (görsel boru hattı) | `[ ]` | 3 |
| Workflow içe/dışa aktarma | `[ ]` | 3 |
| Ayarların diske kalıcı yazımı | `[x]` | 1 |
| Taşınabilir mod | `[ ]` | 6 |

### 12.1 CLI argümanları

| ShareX | Kestrel | Durum |
|---|---|:-:|
| `"dosya/URL yolu"` | `kestrel upload <yol>` | `[ ]` |
| `-workflow "ad"` | `kestrel run "<ad>"` | `[ ]` |
| `-task "ad"` | `kestrel upload <yol> --task "<ad>"` | `[ ]` |
| `-RectangleRegion` vb. hotkey adları | `kestrel capture region` | `[ ]` |
| `-portable` / `-p` | `--portable` | `[ ]` |
| `-silent` / `-s` | `--silent` | `[ ]` |
| `-multi` / `-m` | `--multi` | `[ ]` |
| `-sandbox` | `--ephemeral` | `[ ]` |
| `-autoclose` | `--autoclose` | `[ ]` |
| `-nohotkeys` | `--no-hotkeys` | `[ ]` |
| `-customuploader "<.sxcu>"` | `kestrel import <dosya>` | `[ ]` |
| `-imageeffect "<.sxie>"` | aynı komut | `[ ]` |
| `-ImageEditor`, `-OCR`, `-QRCode`, `-HashCheck`, `-Metadata`, `-PinToScreen`, `-VideoConverter` | `kestrel tool <ad> <yol>` | `[ ]` |

Tümü faz 6.

### 12.2 Sistem entegrasyonu

| Özellik | Durum | Faz | Not |
|---|:-:|:-:|---|
| Sistem tepsisi menüsü | `[x]` | 1 | |
| Dosya yöneticisi bağlam menüsü | `[ ]` | 6 | Quick Action / shell verb / `.desktop` |
| `.sxcu` / `.sxie` dosya ilişkilendirme | `[ ]` | 6 | |
| URL şeması `kestrel://` | `[ ]` | 6 | |
| Sürükle-bırak (tepsiye dosya bırak) | `[ ]` | 3 | |
| Bildirimler + tıklama eylemleri | `[ ]` | 3 | |
| Otomatik güncelleme | `[ ]` | 6 | Tauri updater |
| Başlangıçta çalıştır | `[ ]` | 6 | |
| Tarayıcı uzantısı (native messaging) | `[ ]` | 6+ | Topluluk |
| Politika/yönetim (registry karşılığı) | `[ ]` | 6 | `DisableUpload`, `DisableUpdateCheck` vb. |

---

## 13. Geçmiş ve kütüphane

| Özellik | Durum | Faz |
|---|:-:|:-:|
| Yakalama geçmişi (SQLite) | `[ ]` | 3 |
| Görsel geçmişi (ızgara) | `[ ]` | 3 |
| Tarihe göre bölümleme | `[ ]` | 3 |
| Filtreler (tür, hedef, tarih, yüklendi mi) | `[ ]` | 3 |
| Tam metin arama | `[ ]` | 3 |
| OCR metninde arama | `[ ]` | 5 |
| Toplu işlemler | `[ ]` | 3 |
| Sürükle-bırak dışa aktarma | `[ ]` | 3 |
| Hızlı önizleme | `[ ]` | 3 |
| ShareX `History.xml` içe aktarma | `[ ]` | 3 |

---

## 14. Ayarlar yüzeyleri

| Sekme | İçerik | Durum |
|---|---|:-:|
| Genel | Başlangıçta aç, tepsi, tema, dil, güncelleme | `[ ]` faz 6 |
| Kısayollar | Tüm workflow kısayolları + çakışma tespiti | `[x]` faz 1 |
| Yakalama | Overlay davranışı, imleç, gecikme, ses | `[ ]` faz 2 |
| Kayıt | Codec, fps, kalite, ses, imleç efektleri | `[ ]` faz 4 |
| Editör | Varsayılan araç, renkler, font, davranışlar | `[ ]` faz 2 |
| Hedefler | Servis listesi + yapılandırma + custom uploader | `[ ]` faz 3 |
| Dosyalar | Kaydetme yeri, adlandırma, format, kalite | `[~]` faz 1 |
| Eylemler | Harici program tanımları | `[ ]` faz 5 |
| Gizlilik | İzin durumları, veri temizleme, telemetri | `[~]` faz 1 |
| Gelişmiş | Taşınabilir mod, config klasörü, log, JSON düzenleyici | `[ ]` faz 6 |

---

## 15. Kapsam dışı (`[-]`)

| ShareX özelliği | Gerekçe |
|---|---|
| Windows registry politikaları (birebir) | Platform bazlı politika dosyasıyla değiştirildi |
| Inno Setup kurulum argümanları | Platform paketleyicileri kullanılıyor (dmg/msi/AppImage) |
| DNS Changer | ShareX'ten de kaldırıldı |
| Windows OCR dil paketi kurulum akışı | `ocrs` ile gereksiz |
| .NET bağımlılık kontrolü | İlgisiz |
| Ölü servisler (Google+, eski barındırıcılar) | Custom uploader ile eklenebilir |

---

## Özet

| Bölüm | Toplam madde | Yapıldı | Kısmi | Kalan |
|---|:-:|:-:|:-:|:-:|
| Yakalama yöntemleri | 14 | 6 | 0 | 8 |
| Overlay | 31 | 11 | 1 | 19 |
| Editör | 33 | 13 | 1 | 19 |
| Efektler | ~40 | 0 | 0 | ~40 |
| After-capture / after-upload | 28 | 3 | 0 | 25 |
| Yükleme yöntemleri | 8 | 0 | 0 | 8 |
| Hedefler | ~80 servis + motor | 0 | 0 | tümü |
| Dosya adı token'ları | 37 | 37 | 0 | 0 |
| Araçlar | 25 | 0 | 0 | 25 |
| Workflow / otomasyon | 25 | 7 | 1 | 17 |
| Geçmiş | 10 | 0 | 0 | 10 |
| Ayarlar | 10 | 1 | 2 | 7 |

Faz 1 tamam. Faz 2 sürüyor: anotasyon modeli, geçmiş, renderer ve on bir aracın
tuvali hazır. Kalan büyük parçalar metin araçları (font altyapısı), kırpma/kes,
güzelleştirme ve uploader motoru (faz 3).
