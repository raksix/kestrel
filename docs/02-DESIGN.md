# Kestrel — tasarım sistemi ve arayüz spec'i

---

## 1. Tasarım tezi

ShareX işlevsel olarak eşsiz ama görsel olarak bir kontrol paneli: on beş sekmeli ayar pencereleri, iç içe ağaç menüler, yüzlerce onay kutusu. Güç kullanıcıları sever, yeni kullanıcılar boğulur.

> **Tez: ShareX'in gücünü kaybetmeden, ilk beş dakikayı bir CleanShot kadar kolay yap.**

### İlke 1 — Aşamalı açığa çıkarma, silme yok
Hiçbir ShareX ayarı silinmez, katmanlanır. Her ayar ekranı üç seviyeli: `Temel` (5–8 kontrol) → `Gelişmiş` (katlanabilir) → `Uzman` (JSON düzenleyici). Arama tüm seviyelerde çalışır; kullanıcı bir ayarın nerede olduğunu ezberlemek zorunda kalmaz.

### İlke 2 — Workflow kahramandır
ShareX'te workflow'lar "Hotkey settings" içine gömülü bir tablo. Kestrel'de ana ekranın kendisi. Her workflow bir kart: adı, kısayolu, ne yaptığını gösteren görsel zincir. Bu, uygulamanın zihinsel modelini bir bakışta öğretir.

### İlke 3 — Yakalama anı kutsaldır
Kısayola basmakla overlay'in görünmesi arasında **100ms'den az** olmalı. Overlay'de her karar klavyeyle yapılabilmeli; fare zorunlu olmamalı.

### İlke 4 — Sonuç sessizce ulaşılabilir olsun
Bildirim yağmuru yok. Ekranın bir köşesinde tek yüzen kart: sürükle → başka uygulamaya bırak · tıkla → düzenle · kaydır → yoksay · üzerine gel → hızlı eylemler. Süre dolunca sessizce kaybolur.

### İlke 5 — Her platformda evinde hisset, hiçbirini taklit etme
Kestrel üç platformda da **kendi** tutarlı görsel dilini kullanır — ama platform davranışlarına uyar: kısayol modifier'ları, pencere düğmesi konumu, menü çubuğu vs. tepsi, dosya diyalogları, sistem teması ve vurgu rengi. Görünüm bizim, davranış platformun.

---

## 2. Görsel dil

### 2.1 Renk

Kestrel kendi marka rengini dayatmaz — **sistem vurgu rengini okur** ve onu accent olarak kullanır. Bu, uygulamayı üç platformda da anında "yerli" hissettirir. Kullanıcı isterse sabit bir renge kilitleyebilir.

Tüm renkler CSS değişkeni; açık/koyu tema otomatik.

```css
--bg-canvas      /* pencere zemini */
--bg-surface     /* kart, panel */
--bg-raised      /* açılır menü, popover */
--bg-inset       /* girdi alanı, kod bloğu */

--text-primary
--text-secondary
--text-muted

--border         /* varsayılan hairline */
--border-strong  /* hover, odak */

--accent         /* sistemden okunur */
--accent-fg      /* accent üzerindeki metin */

--success --warning --danger
--record         /* #FF453A sabit — evrensel olmalı, sistem rengine bağlanmaz */
```

**Overlay renkleri sistemden bağımsızdır** (rastgele içerik üstünde çizilir):
- Karartma `rgb(0 0 0 / 0.42)`
- Seçim çerçevesi: beyaz 1px + siyah %30 dış hat — her arka planda görünür
- Boyut rozeti: siyah %78 zemin, beyaz metin
- Büyüteç çerçevesi: beyaz 2px

### 2.2 Tipografi

Sistem font yığını — her platformda o platformun metni gibi görünür:

```css
--font-sans: ui-sans-serif, -apple-system, "Segoe UI Variable Text",
             "Segoe UI", Inter, "Noto Sans", sans-serif;
--font-mono: ui-monospace, "SF Mono", "Cascadia Code", "JetBrains Mono", monospace;
```

| Rol | Boyut | Ağırlık |
|---|---|---|
| Bölüm başlığı | 15 | 600 |
| Gövde | 13 | 400 |
| İkincil | 11 | 400 |
| Rozet / sayı | 11 | 500, `tabular-nums` |
| Kod / token | 12 | 400, mono |

Sabit yükseklikli satır yok — kullanıcı sistem metin boyutunu büyütürse her şey ölçeklenir.

### 2.3 Ölçü, köşe, derinlik

8px ızgara: `4 · 8 · 12 · 16 · 24 · 32`

| Eleman | Yarıçap |
|---|---|
| Kart | 12 |
| Kart içi eleman | 8 |
| Buton / girdi | 8 |
| Overlay araç çubuğu | kapsül |
| Rozet | 6 |

**Derinlik:** Gölge yerine **kenarlık ve yüzey tonu**. Yalnızca gerçekten yüzen şeyler (popover, yakalama sonrası kart, overlay araç çubuğu) hafif gölge alır. Gradyan yok, glow yok, doku yok.

**Bulanık arka plan (backdrop-filter):** yalnızca yüzen ve geçici yüzeylerde. Editör tuvali ve ayar sayfaları düz — yoğun içerikte bulanıklık okunabilirliği düşürür. `prefers-reduced-transparency` açıkken tümü opaklaşır.

### 2.4 İkonografi

Tek bir ikon seti: **Lucide** (outline, 16/20/24px). Özel ikon yalnızca uygulama logosu ve tepsi ikonları için.

Araç eşlemeleri: seç `mouse-pointer-2` · dikdörtgen `square` · elips `circle` · çizgi `minus` · ok `arrow-up-right` · serbest `pen-tool` · metin `type` · balon `message-square` · adım `circle-1` · görsel `image` · emoji `smile` · imleç `mouse-pointer` · vurgu `highlighter` · silgi `eraser` · bulanık `droplet` · piksel `grid-3x3` · büyüteç `search` · spot `flashlight` · kırp `crop` · kes `scissors`

**Tepsi ikonu:** Tek renkli şablon, platform başına doğru boyutta (macOS 22pt template, Windows 16px, Linux 24px). Durumlar: boşta · yükleniyor (ilerleme halkası) · kayıtta (kırmızı nokta) · hata.

**Uygulama ikonu:** Kerkenez (kestrel) siluetinden soyutlanmış bir kadraj köşesi. macOS'ta yuvarlak köşeli kare, Windows'ta tam kare, Linux'ta serbest — üçü ayrı üretilir.

### 2.5 Hareket

| Etkileşim | Animasyon |
|---|---|
| Overlay açılış | **Yok** — anında |
| Overlay kapanış | 120ms fade |
| Yakalama sonrası kart giriş | 320ms yay, köşeden kayarak |
| Kart çıkış | 240ms ölçek + fade |
| Araç seçimi | 140ms |
| Panel açılış | 200ms |

`prefers-reduced-motion` açıkken tüm yaylar 100ms linear fade'e düşer, hiçbir şey kaymaz.

---

## 3. Ekran spec'leri

### 3.1 Tepsi paneli — birincil giriş noktası

Tepsi ikonuna tıklandığında açılan panel, 320px.

```
┌──────────────────────────────────────┐
│  Kestrel                        ⚙︎    │
├──────────────────────────────────────┤
│  ⌘⇧2  Bölge yakala                   │
│  ⌘⇧3  Tüm ekran                      │
│  ⌘⇧4  Pencere                        │
│  ⌘⇧5  Ekran kaydı                    │
├──────────────────────────────────────┤
│  Workflow'lar                     ›  │
│   ▸ Imgur'a yükle              ⌘⇧U   │
│   ▸ Blog için düzenle          ⌘⇧B   │
├──────────────────────────────────────┤
│  Son yakalamalar                     │
│  [▪] [▪] [▪] [▪]                     │
├──────────────────────────────────────┤
│  Araçlar                          ›  │
│  Kütüphane…                          │
└──────────────────────────────────────┘
```

Kısayollar `tabular-nums` ile hizalı ve platforma göre yazılır (`⌘⇧2` / `Ctrl+Shift+2`). Son yakalamalar şeridi doğrudan sürüklenebilir. Yükleme varsa üstte ince ilerleme çubuğu. Ok tuşları + `Enter` ile tam klavye erişimi.

### 3.2 Bölge yakalama overlay'i

Katmanlar (alttan üste):
1. Donmuş ekran görüntüsü
2. Karartma (seçim hariç)
3. Yapışma vurgusu — hover edilen pencerenin çerçevesi
4. Seçim — beyaz 1px + 8 tutamak
5. Anotasyon şekilleri
6. HUD — büyüteç, boyut rozeti, artı imleç
7. Araç çubuğu — alt orta, yüzen kapsül

**Araç çubuğu düzeni** (gruplar ayırıcıyla):
```
şekiller │ metin │ gizleme │ kırpma │ ⋯ │ renk │ geri/ileri │ onay
```
Seçili araç accent dolgulu. Araç seçilince altında ikinci sıra olarak o aracın seçenekleri açılır (renk, kalınlık, opaklık). Araç çubuğu seçimi kapatacaksa otomatik üste taşınır. `Tab` ile tamamen gizlenir.

**Büyüteç:** imlecin 24px sağ altında, 120×120, 8× zoom, piksel ızgarası, merkezde artı, altta hex kodu. Ekran kenarında karşı tarafa geçer.

**Boyut rozeti:** seçimin sol üstünde (yer yoksa içinde), `1280 × 720`.

**İpucu şeridi:** ilk 3 kullanımda altta görünür — `Space tam ekran · Tab araçları gizle · Esc iptal`. Sonra kaybolur, `?` ile geri gelir.

### 3.3 Yakalama sonrası kart

200×140, yüzen, köşede.

```
╭──────────────────────╮
│    [küçük resim]     │  tıkla → editör
│                      │  sürükle → dışa aktar
├──────────────────────┤
│  ✎  ↑  ⧉  📌  ⌫   ⋯ │
╰──────────────────────╯
```

Yükleme sürerken küçük resmin üstünde dairesel ilerleme; bitince "URL kopyalandı" rozeti. Kaydırarak kapat. Üst üste yakalamalar deste olur, sayaç rozeti gösterir.

### 3.4 Ana pencere

Üç kolon: kenar çubuğu (180px) · içerik · denetçi (280px, gizlenebilir).

**Kenar çubuğu:**
```
YAKALAMA          WORKFLOW'LAR        ARAÇLAR
  Kütüphane         Tüm workflow'lar    Renk seçici
  Yükleme kuyruğu   ▸ Bölge → Imgur     Cetvel
                    ▸ Blog görüntüsü    OCR
                    + Yeni workflow     QR kod
                                        …
```

**Araç çubuğu:** arama · görünüm değiştirici · filtre · paylaş · denetçi aç/kapa.

Pencere kontrolleri platforma göre konumlanır (macOS solda, Windows/Linux sağda) — özel başlık çubuğu kullanıldığı için bu elle yönetilir.

### 3.5 Kütüphane

160px küçük resimler, tarihe göre bölümlenmiş (`Bugün`, `Dün`, `Bu hafta`, `Ağustos 2026`). Hover ile hızlı eylemler. Liste görünümü ShareX'in History tablosunun karşılığı: tarih, ad, boyut, hedef, URL.

Arama: dosya adı + URL + **OCR metni** + etiketler. Filtre çipleri: tür, hedef, tarih aralığı, yüklendi/yüklenmedi. Toplu seçim ve işlemler. Sanal liste (10 binlerce öğede akıcı).

Boş durum: kısayol ipucu + "ilk yakalamanı yap" butonu.

### 3.6 Workflow düzenleyici — tasarımın kalbi

ShareX'in en güçlü ama en gizli özelliğini görünür kılar.

```
┌────────────────────────────────────────────────────────────┐
│  Ad: [ Blog için ekran görüntüsü ]   Kısayol: [ ⌘⇧B ] ✓    │
├────────────────────────────────────────────────────────────┤
│   ╭─────────╮   ╭─────────╮   ╭─────────╮   ╭─────────╮   │
│   │ YAKALA  │ → │ İŞLE    │ → │ YÜKLE   │ → │ SONRASI │   │
│   │ Bölge   │   │ Editör  │   │ Imgur   │   │ URL     │   │
│   │         │   │ Gölge   │   │         │   │ kopyala │   │
│   ╰─────────╯   ╰─────────╯   ╰─────────╯   ╰─────────╯   │
├────────────────────────────────────────────────────────────┤
│  Dosya adı: [%y-%mo-%d_%h-%mi-%s] → 2026-08-29_14-32-07    │
├────────────────────────────────────────────────────────────┤
│  ▸ Gelişmiş (kalite, efektler, eylemler, bildirimler)      │
└────────────────────────────────────────────────────────────┘
```

Kutulara tıklayınca ayarları denetçide açılır. Aşamalar sürüklenerek sıralanabilir (ShareX'te görev sırası önemlidir). Dosya adı alanı **canlı önizleme** yapar — token'ları öğrenmeyi kolaylaştırır. Sağ üstte "Test et" — örnek görselle çalıştırır.

### 3.7 Ayarlar

Sekmeler: Genel · Kısayollar · Yakalama · Kayıt · Editör · Hedefler · Dosyalar · Eylemler · Gizlilik · Gelişmiş

Her sekmede üstte arama; `Temel` kontroller doğrudan, `Gelişmiş` katlanabilir bölümde.

**Hedefler sekmesi:** servisler logo kartları ızgarası, yapılandırılmışlar onay rozetli. Üstte "Custom uploader ekle" + `.sxcu` sürükle-bırak alanı.

**Gizlilik sekmesi:** her platformun izin durumu, hangi özelliğin neden hangi izne ihtiyaç duyduğu, tek tıkla sistem ayarına gitme, veri temizleme. Telemetri varsayılan **kapalı**.

### 3.8 Editör penceresi

```
┌─────────────────────────────────────────────────────────────┐
│ geri/ileri · kaydet · kopyala · yükle · paylaş               │
├──────┬──────────────────────────────────────────┬───────────┤
│ A    │                                          │ Şekil     │
│ R    │              TUVAL                        │ ayarları  │
│ A    │        (düz, dama zemin)                  │           │
│ Ç    │                                          │ ─────────  │
│ L    │                                          │ Katmanlar │
│ A    │                                          │           │
│ R    │                                          │           │
├──────┴──────────────────────────────────────────┴───────────┤
│ ⌄ Arka plan: gölge · padding · köşe · oran · duvar kâğıdı   │
├─────────────────────────────────────────────────────────────┤
│  100%  ⊖ ──●── ⊕     1920×1080      [İptal]  [Bitti ⏎]     │
└─────────────────────────────────────────────────────────────┘
```

Tuval **düz** — bulanıklık yok, şeffaflık için standart dama deseni. Sağ denetçide seçili şeklin özellikleri + **katman listesi** (ShareX'te yok).

---

## 4. Onboarding

5 ekran, 600×440.

1. **Hoş geldin** — ne yaptığı, kısa döngüsel demo.
2. **İzinler** — platforma göre değişir. macOS'ta Ekran Kaydı izni (neden gerektiği açıklanır, buton doğrudan sistem ayarına götürür, izin verilince otomatik ilerler). Windows'ta izin yok, bu ekran atlanır. Linux'ta Wayland ise portal davranışı anlatılır.
3. **Kısayollar** — önerilen set, her biri tek tıkla değiştirilebilir, sistem çakışmaları kırmızı işaretlenir.
4. **Nereye kaydedilsin** — klasör + varsayılan hedef (yalnızca yerel / Imgur / özel servis).
5. **Deneyelim** — interaktif ilk yakalama.

**Kritik:** izin istemeden önce mutlaka *neden* açıklanır. "Bu uygulama ekranımı neden görmek istiyor" sorusu cevapsız bırakılmaz.

---

## 5. Erişilebilirlik

| Gereksinim | Uygulama |
|---|---|
| Ekran okuyucu | Tüm kontroller etiketli (VoiceOver / Narrator / Orca). Canvas için paralel DOM ağacı: her şekil ayrı erişilebilir eleman. |
| Tam klavye erişimi | Her ekran Tab ile gezilebilir. Overlay tamamen klavyeyle kullanılabilir. |
| Reduced motion | Tüm yaylar linear fade'e düşer. |
| Reduced transparency | Bulanık yüzeyler opaklaşır, overlay karartması artar. |
| Yüksek kontrast | Kenarlıklar kalınlaşır, seçim çerçevesi 2px'e çıkar. |
| Metin ölçekleme | Sabit yükseklik yok, `rem` tabanlı. |
| Renk körlüğü | Durum asla tek başına renkle verilmez — ikon + metin eşlik eder (kaydırmalı yakalama göstergesi ✓/!/✕ ile birlikte). |
| Odak görünürlüğü | Her etkileşimli elemanda net odak halkası. |

CI'da `axe-core` ile otomatik kontrol, sürüm öncesi manuel ekran okuyucu geçişi.

---

## 6. Lokalizasyon

Başlangıç: Türkçe, İngilizce. `i18next`, tüm metinler dışarıda, kodda sabit string yok. RTL desteği baştan yapıda (`inline-start`/`inline-end`, asla `left`/`right`). Kısayol gösterimi platform ve klavye düzenine duyarlı.

---

## 7. Tasarım teslimatları

| Teslimat | Format | Faz |
|---|---|---|
| Token katmanı (CSS değişkenleri + Tailwind teması) | `src/design/` | 0 |
| Bileşen kütüphanesi | React + Storybook | 0–1 |
| Uygulama ikonu (3 platform varyantı) | SVG → platform formatları | 1 |
| Tepsi ikon setleri | SVG şablon | 1 |
| Overlay etkileşim prototipi | Çalışan kod | 1 |
| Onboarding metinleri | i18n katalog | 6 |

**Not:** Ayrı bir Figma dosyası tutmuyoruz. Tasarım doğrudan kodda ve Storybook'ta yaşıyor — senkron kalma sorunu tamamen ortadan kalkıyor. Figma yalnızca uygulama ikonu ve pazarlama görselleri için.
