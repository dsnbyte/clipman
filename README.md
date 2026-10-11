# Clipman — Lightweight Clipboard Manager untuk Ubuntu

Clipman adalah aplikasi clipboard manager yang dibangun khusus untuk Ubuntu menggunakan **Rust** dan framework UI GPU-accelerated **GPUI** (dari pembuat editor Zed).

---

## Fitur Utama

- **Lightweight & Efisien**:
  - Background daemon berjalan 24/7 hanya menggunakan **~11 MB RAM**.
  - Menggunakan arsitektur dual-process: saat shortcut ditekan (`Ctrl+Alt+V`), dialog GPUI muncul instan (<80ms). Ketika dialog ditutup atau item dipilih, proses UI keluar dan seluruh alokasi GPU/memori dibebaskan kembali.
  - Zero-polling: Menggunakan event XFixes Selection Notification via `x11rb`, tidak membebani CPU saat idle.
- **Dukungan Teks, Gambar, dan File**:
  - **Teks**: Menyimpan teks biasa, kode sumber, link dengan preview rapi.
  - **File**: Mendukung penyalinan file dari GNOME Files (Nautilus) maupun aplikasi lain melalui `x-special/gnome-copied-files` dan `text/uri-list`.
  - **Gambar**: Menyimpan format `image/png` langsung ke disk blob lengkap dengan thumbnail cepat.
  - **Privasi**: Otomatis mendeteksi hint password manager (`x-kde-passwordManagerHint`) dan fitur pause capture.
- **Tray Icon di Title Bar / Top Bar Ubuntu**:
  - Menggunakan protokol `StatusNotifierItem` / `AppIndicator` (`ksni`).
  - Menampilkan riwayat clipboard terkini langsung di menu klik tray.
  - Menu: Open Clipman, Pause Capturing, Settings, Quit.
- **Tampil di Tengah Layar (Centered Dialog)**:
  - Dialog pop-up GPUI otomatis tampil di tengah layar monitor utama.
  - Tampilan otomatis mengikuti tema sistem Ubuntu (Dark / Light mode).
- **Live Search & Filter**:
  - Kolom pencarian instan (cukup ketik langsung untuk mencari riwayat).
  - Tab kategori: **All**, **Pinned**, **Text**, **Images**, **Files**.
- **Keyboard Shortcuts Lengkap**:
  - `↑` / `↓` / `Ctrl+J` / `Ctrl+K`: Navigasi item.
  - `Enter`: Salin item terpilih ke clipboard dan tutup dialog.
  - `Ctrl+P`: Pin / unpin item (item yang di-pin tidak akan terhapus saat rotasi riwayat).
  - `Del`: Hapus item dari riwayat.
  - `Tab` / `Shift+Tab`: Pindah tab kategori filter.
  - `Esc`: Tutup dialog.
- **Pengaturan Global Shortcut**:
  - Terintegrasi langsung dengan GNOME Settings (`gsettings` media-keys).
  - Default shortcut: `Ctrl + Alt + V`.
  - Dapat diubah langsung dari menu Settings di dalam aplikasi atau melalui CLI.

---

## Struktur Proyek

```text
clipman/
├── Cargo.toml          # Konfigurasi dependensi dan profil rilis teroptimasi
├── assets/icons/       # Vektor SVG ikon (embedded ke dalam binary)
├── src/
│   ├── main.rs         # CLI dispatcher & subcommands
│   ├── model.rs        # Tipe data bersama (Kind, Payload, Filter, Cmd)
│   ├── config.rs       # Konfigurasi persisten (~/.config/clipman/config.toml)
│   ├── store.rs        # SQLite WAL-mode storage & blob thumbnail cache
│   ├── system.rs       # Integrasi Ubuntu GNOME (gsettings shortcut & autostart)
│   ├── ipc.rs          # Komunikasi Unix domain socket (~/clipman.sock)
│   ├── clipboard.rs    # Event-driven clipboard monitor (XFixes & INCR transfer)
│   ├── tray.rs         # StatusNotifierItem / AppIndicator tray icon
│   ├── daemon.rs       # Background daemon runner
│   ├── assets.rs       # GPUI AssetSource implementation
│   └── ui.rs           # Tampilan UI GPUI (Centered dialog, list, search, settings)
```

---

## Cara Kompilasi & Menjalankan

### Persyaratan Sistem (Ubuntu 24.04 / 22.04 LTS)

Paket pengembangan dasar yang dibutuhkan sudah tersedia di Ubuntu:
```bash
sudo apt install -y build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev
```

### Build Binary Rilis

```bash
cargo build --release
```

Binary hasil kompilasi berada di: `target/release/clipman`.

---

## Penggunaan

### 1. Pasang Otomatis (Desktop Launcher, Autostart & Shortcut)

Jalankan perintah install sekali:
```bash
./target/release/clipman install
```
Perintah ini akan:
- Mendaftarkan entri aplikasi di menu Ubuntu (`~/.local/share/applications/clipman.desktop`).
- Menambahkan clipman ke autostart saat login (`~/.config/autostart/clipman.desktop`).
- Mendaftarkan shortcut default **`Ctrl + Alt + V`** di GNOME Keyboard Shortcuts.

### 2. Menjalankan Manual / di Background

```bash
# Menjalankan daemon di background
./target/release/clipman daemon &

# Membuka dialog UI (atau lewat shortcut Ctrl+Alt+V)
./target/release/clipman toggle

# Membuka langsung layar Settings
./target/release/clipman settings

# Menghentikan background service
./target/release/clipman quit

# Menghapus instalasi launcher & shortcut
./target/release/clipman uninstall
```
