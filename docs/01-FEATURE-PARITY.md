# Kestrel — ShareX özellik paritesi matrisi

**Durum kodları**
🟢 Birebir · 🔵 Uyarlanmış · 🟣 İyileştirilmiş · 🟡 Kısmi · 🔴 Kapsam dışı · ⚫ Yeni (ShareX'te yok)

**Platform kolonları:** M = macOS · W = Windows · X = Linux/X11 · Y = Linux/Wayland

---

## 1. Yakalama yöntemleri

| ShareX | Durum | M | W | X | Y | Not |
|---|---|:-:|:-:|:-:|:-:|---|
| Fullscreen | 🟢 | ✅ | ✅ | ✅ | ✅ | Tüm ekranlar birleşik |
| Active window | 🟢 | ✅ | ✅ | ✅ | ⚠️ | Wayland pencere listesi vermiyor |
| Active monitor | 🟢 | ✅ | ✅ | ✅ | ✅ | |
| Window menu | 🟢 | ✅ | ✅ | ✅ | ❌ | Küçük resimli liste |
| Monitor menu | 🟢 | ✅ | ✅ | ✅ | ✅ | |
| Region | 🟢 | ✅ | ✅ | ✅ | ✅ | Donmuş kare + overlay |
| Region (Light) | 🔵 | ✅ | ✅ | ✅ | ✅ | Overlay teması "sade" |
| Region (Transparent) | 🔵 | ✅ | ✅ | ✅ | ✅ | Overlay teması "şeffaf" |
| Last region | 🟢 | ✅ | ✅ | ✅ | ✅ | |
| Custom region | 🟢 | ✅ | ✅ | ✅ | ✅ | Adlandırılmış sabit bölgeler |
| Screen recording | 🟢 | ✅ | ✅ | ✅ | ✅ | ffmpeg sidecar |
| Screen recording (GIF) | 🟢 | ✅ | ✅ | ✅ | ✅ | |
| Scrolling capture | 🟡 | ⚠️ | ✅ | ⚠️ | ❌ | Platform otomasyonuna bağlı |
| Auto capture | 🟢 | ✅ | ✅ | ✅ | ✅ | |
| Gecikmeli yakalama | ⚫ | ✅ | ✅ | ✅ | ✅ | 3/5/10 sn geri sayım |
| Her ekran ayrı dosya | ⚫ | ✅ | ✅ | ✅ | ✅ | |

## 2. Bölge overlay'i

| Özellik | Durum | Not |
|---|---|---|
| Dikdörtgen / elips / serbest bölge | 🟢 | |
| Çoklu bölge modu | 🟢 | `Enter` / çift tık |
| Büyüteç + piksel ızgarası | 🟢 | Tekerlekle boyut |
| Artı imleç | 🟢 | |
| Canlı boyut/konum göstergesi | 🟢 | |
| Pencereye yapışma | 🟢 | Wayland hariç |
| Ön ayar oranlara kilit | 🟢 | |
| Orantılı yeniden boyutlandırma | 🟢 | |
| 21 anotasyon aracı overlay içinde | 🟢 | Editörle ortak motor |
| Monitör kısayolları (`Space`/`1-0`/`~`) | 🟢 | |
| Konum-boyut kopyala | 🟢 | |
| Z-sırası tuşları | 🟢 | |
| Undo/redo | 🟢 | |
| Görünür araç çubuğu | ⚫ | ShareX'te araçlar gizli kısayolda |
| Overlay içi renk seçici | 🟢 | |

## 3. Editör (20 araç)

Seç `V` · Dikdörtgen `R` · Elips `E` · Çizgi `L` · Ok `A` · Serbest `F` · Metin `T` · Balon `O` · Adım `N` · Görsel `I` · Emoji `J` · İmleç `K` · Vurgu `H` · Akıllı silgi `W` · Bulanık `B` · Piksel `P` · Büyüteç `M` · Spot `S` · Kırp `C` · Kes `U` — **hepsi 🟢, üç platformda**

| Ek | Durum |
|---|---|
| Yeniden düzenlenebilir `.kestrel` dosyası | 🟣 |
| Katman listesi paneli | ⚫ |
| Otomatik hassas veri gizleme | ⚫ |
| 3D perspektif eğimi | ⚫ |

**Eylem kısayolları** (`Enter`/`Esc`/`Mod+S`/`Mod+U`/`Mod+P`/`Mod+Shift+F`…) — 🟢
**Arka plan araçları** (margin, padding, akıllı padding, köşe, gölge, oran, şeffaf/düz/gradyan/görsel/duvar kâğıdı) — 🟢
**Editör seçenekleri** (tema, vurgu rengi, pencere durumu, çıkış onayı, sığdır, hızlı kırpma, otomatik kapat, otomatik kopyala) — 🟢

## 4. After-capture görevleri (22)

Hızlı görev menüsü · "Yakalama sonrası" penceresi · Güzelleştir · Efekt uygula · Editörde aç · Panoya kopyala · Ekrana sabitle · Yazdır · Dosyaya kaydet · Farklı kaydet · Küçük resim kaydet · Eylemleri çalıştır · Dosyayı panoya kopyala · Dosya yolunu kopyala · Klasör yolunu kopyala · Analiz et · QR tara · "Yükleme öncesi" penceresi · Yükle · Yerelden sil — **🟢**

| ShareX | Durum | Not |
|---|---|---|
| Show file in explorer | 🔵 | Finder / Explorer / dosya yöneticisi |
| Recognize text (OCR) | 🟣 | Kurulum gerektirmez |
| — | ⚫ | Shortcuts / betik çalıştır |

## 5. After-upload görevleri (6)

"Yükleme sonrası" penceresi · URL kısalt · URL paylaş · URL kopyala · URL aç · QR göster — **hepsi 🟢**

## 6. Yükleme yöntemleri

Dosya yükle · Klasör yükle · Panodan yükle · Metin yükle · URL'den yükle · Sürükle-bırak · URL kısalt — **🟢**
İzleme klasörü — 🔵 (`notify` crate)
Dosya yöneticisi bağlam menüsünden yükleme — ⚫

## 7. Hedefler

| Kategori | Durum | Faz |
|---|---|---|
| **Custom uploader (.sxcu)** | 🟢 | 3 |
| Imgur | 🟢 | 3 |
| S3 uyumlu (AWS · R2 · B2 · Spaces · MinIO) | 🟢 | 3 |
| Google Cloud Storage | 🟢 | 4 |
| Azure Storage | 🟢 | 5 |
| Dropbox · Google Drive | 🟢 | 3 |
| OneDrive · Box · pCloud · Mega | 🟢 | 4-5 |
| FTP · FTPS · SFTP | 🟢 | 3 |
| Gist · GitLab snippet | 🟢 | 3 |
| Pastebin · Hastebin · Paste.ee | 🟢 | 3 |
| bit.ly · is.gd · tinyurl · yourls · Kutt · Polr | 🟢 | 4 |
| Discord webhook | 🟢 | 3 |
| Slack · Telegram · e-posta | 🟢 | 4 |
| Mastodon · Bluesky | ⚫ | 4 |
| Twitter/X | 🟡 | 5 |
| YouTube · Streamable | 🟡 | 5 |
| Ölü/terk edilmiş servisler | 🔴 | — |

## 8. Custom uploader syntax

`{response}` · `{responseurl}` · `{header:}` · `{json:}` · `{xml:}` · `{regex:}` · `{input}` · `{filename}` · `{random:}` · `{select:}` · `{inputbox:}` · `{outputbox:}` · `{base64:}` · `\` kaçış · `.sxcu` import/export — **hepsi 🟢**
İstek/yanıt inspector — ⚫

## 9. Dosya adı token'ları

`%t` `%pn` `%y` `%yy` `%mo` `%mon` `%mon2` `%w` `%w2` `%wy` `%d` `%h` `%mi` `%s` `%ms` `%pm` `%unix` `%i` `%ia` `%iAa` `%ib` `%ix` `%rn` `%ra` `%rna` `%rx` `%guid` `%radjective` `%ranimal` `%remoji` `%rf` `%width` `%height` `%un` `%uln` `%cn` `%n` — **hepsi 🟢**

Ekler: `%app` · `%ocr` — ⚫ · Canlı önizleme — ⚫

## 10. Araçlar (24)

| Araç | Durum | Not |
|---|---|---|
| Renk seçici · Ekran renk seçici · Cetvel | 🟢 | |
| Ekrana sabitle | 🟢 | Tüm kısayollarıyla |
| Görsel editörü · Güzelleştirici · Efektler · Görüntüleyici | 🟢 | |
| Arka plan kaldırıcı | 🔵 | `rembg` ONNX modeli — ShareX ile aynı yaklaşım, uygulama içi indirme |
| Karşılaştırıcı · Birleştirici · Bölücü · Küçük resim üretici | 🟢 | |
| Video dönüştürücü · Video küçük resmi | 🟢 | ffmpeg |
| Görsel analizi | 🟢 | |
| OCR | 🟣 | `ocrs` + opsiyonel native motor |
| QR kod (oluştur + tara) | 🟢 | |
| Hash kontrolü | 🟢 | |
| Metadata (görüntüle + temizle) | 🟢 | |
| Dizin indeksleyici | 🟢 | HTML/TXT/XML/JSON |
| Pano görüntüleyici | 🟢 | |
| Kenarlıksız pencere | 🟡 | Wayland'de yok |
| Pencere inceleme | 🟡 | Wayland'de yok |
| Monitör testi | 🟢 | |

## 11. Workflow & otomasyon

| ShareX | Durum | Not |
|---|---|---|
| Workflow sistemi | 🟢 | Çekirdek kavram korunuyor |
| Workflow başına ayar geçersiz kılma | 🟢 | |
| Global kısayollar | 🟡 | Wayland'de portal / DE ayarı |
| Kısayolları geçici devre dışı bırak | 🟢 | |
| CLI argümanları | 🔵 | Alt komut yapısı |
| `-workflow` / `-task` | 🟢 | |
| `-portable` | 🟢 | |
| `-silent` / `-autoclose` / `-nohotkeys` | 🟢 | |
| `-sandbox` | 🟢 | `--ephemeral` |
| `.sxcu` / `.sxie` dosya ilişkilendirme | 🟢 | |
| Registry politikaları | 🔵 | Platform bazlı politika dosyası |
| Explorer bağlam menüsü | 🔵 | Üç platformda karşılığı |
| Tarayıcı uzantısı | 🟡 | Faz 6+ / topluluk |
| URL şeması `kestrel://` | ⚫ | |
| Tray'e sürükle-bırak | ⚫ | |

## 12. Uygulama yüzeyleri

| ShareX | Durum | Not |
|---|---|---|
| Ana pencere | 🔵 | Kütüphane + workflow merkezli yeniden tasarım |
| Tepsi ikonu | 🟢 | |
| Geçmiş | 🟣 | SQLite, OCR metninde arama |
| Görsel geçmişi | 🟢 | |
| Uygulama / görev / hedef / kısayol ayarları | 🟢 | Aşamalı açığa çıkarma ile |
| Tema / vurgu rengi | 🔵 | Sistem temasını takip |
| Bildirimler + tık eylemleri | 🟢 | |
| Otomatik güncelleme | 🟢 | Tauri updater |
| Taşınabilir mod | 🟢 | |
| Çeviri | 🟢 | TR/EN başlangıç |
| `History.xml` içe aktarma | ⚫ | Windows'tan göç yolu |

## 13. Kapsam dışı (🔴)

| ShareX özelliği | Neden |
|---|---|
| Windows registry politikaları (birebir) | 🔵 olarak platform bazlı uyarlandı |
| Inno Setup argümanları | Platform paketleyicileri kullanılıyor |
| DNS Changer | ShareX'te de kaldırıldı |
| Windows OCR dil paketi kurulumu | `ocrs` ile gereksiz |
| .NET bağımlılık kontrolü | — |

---

## Özet

ShareX'in belgelenmiş özelliklerinin **~%90'ı birebir veya daha iyi** karşılanıyor.

Kalanın dağılımı:
- **Wayland kaynaklı kısıtlar** — protokol tasarımı gereği, hiçbir uygulamanın aşamadığı sınırlar. X11'de tam destek var.
- **Platform otomasyonuna bağlı özellikler** (kaydırmalı yakalama, kenarlıksız pencere) — Windows'ta tam, diğerlerinde kısmi.
- **Ölü servisler** — custom uploader ile kullanıcı tarafından eklenebilir.
- **Windows'a özgü mekanizmalar** — diğer platformlarda karşılığı olan yaklaşımlarla değiştirildi.
