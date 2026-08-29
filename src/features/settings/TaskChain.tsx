import { useEffect, useState } from "react";
import {
  listTasks,
  setTasks,
  type AppSettings,
  type TaskInfo,
  type Workflow,
} from "../../lib/ipc";
import "./settings.css";

/**
 * ShareX's after-capture and after-upload task chains.
 *
 * Two things this deliberately does *not* do.
 *
 * It does not let the chain be reordered. The order is the pipeline — saving
 * has to precede copying the file path, uploading has to precede deleting the
 * file — so it is a property of the tasks, not a preference. Rust re-sorts
 * whatever arrives, and offering drag handles would promise control that does
 * not exist.
 *
 * It does not let an unimplemented task be switched on. Rust reports which
 * tasks it actually performs; the rest are shown greyed out with a reason,
 * because a checkbox that ticks and then does nothing is worse than one that
 * refuses — the user cannot tell "did not run" from "ran and had no effect".
 */

const CAPTURE_LABELS: Record<string, string> = {
  show_quick_task_menu: "Hızlı görev menüsünü göster",
  show_after_capture_window: "Yakalama sonrası penceresini göster",
  beautify_image: "Görseli güzelleştir",
  add_image_effects: "Görsel efektlerini uygula",
  open_in_editor: "Editörde aç",
  copy_image_to_clipboard: "Görseli panoya kopyala",
  pin_to_screen: "Ekrana sabitle",
  print_image: "Yazdır",
  save_image_to_file: "Dosyaya kaydet",
  save_image_to_file_as: "Farklı kaydet…",
  save_thumbnail_image_to_file: "Küçük resmi kaydet",
  perform_actions: "Eylemleri çalıştır",
  copy_file_to_clipboard: "Dosyayı panoya kopyala",
  copy_file_path_to_clipboard: "Dosya yolunu panoya kopyala",
  copy_folder_path_to_clipboard: "Klasör yolunu panoya kopyala",
  show_in_file_manager: "Dosya yöneticisinde göster",
  analyze_image: "Görseli incele",
  scan_qr_code: "QR kodu tara",
  recognize_text: "Metni tanı (OCR)",
  show_before_upload_window: "Yüklemeden önce sor",
  upload_image_to_host: "Sunucuya yükle",
  delete_file_locally: "Yerel dosyayı sil",
};

const UPLOAD_LABELS: Record<string, string> = {
  show_after_upload_window: "Yükleme sonrası penceresini göster",
  shorten_url: "URL'yi kısalt",
  share_url: "URL'yi paylaş",
  copy_url_to_clipboard: "URL'yi panoya kopyala",
  open_url: "URL'yi aç",
  show_qr_code: "QR kodu göster",
};

export default function TaskChain({
  workflows,
  settings,
  onSettingsChanged,
}: {
  workflows: Workflow[];
  settings: AppSettings | null;
  onSettingsChanged: (settings: AppSettings) => void;
}) {
  const [capture, setCapture] = useState<TaskInfo[]>([]);
  const [upload, setUpload] = useState<TaskInfo[]>([]);
  const [target, setTarget] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listTasks()
      .then(([captureTasks, uploadTasks]) => {
        setCapture(captureTasks);
        setUpload(uploadTasks);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const chain = target
    ? workflows.find((w) => w.id === target)?.settings
    : settings?.defaults;

  const enabledCapture = chain?.after_capture ?? [];
  const enabledUpload = chain?.after_upload ?? [];
  const saves = enabledCapture.includes("save_image_to_file");

  const toggle = (kind: "capture" | "upload", id: string, on: boolean) => {
    const next = (list: string[]) =>
      on ? [...list, id] : list.filter((existing) => existing !== id);

    const nextCapture = kind === "capture" ? next(enabledCapture) : enabledCapture;
    const nextUpload = kind === "upload" ? next(enabledUpload) : enabledUpload;

    setTasks(target, nextCapture, nextUpload)
      .then((updated) => {
        onSettingsChanged(updated);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  };

  const row = (
    task: TaskInfo,
    kind: "capture" | "upload",
    labels: Record<string, string>,
    enabled: string[],
  ) => {
    const on = enabled.includes(task.id);
    // A file task with no "save to file" is a chain that cannot work. It is a
    // configuration mistake rather than a failure, so it is a note, not a block.
    const orphaned = on && task.needsSavedFile && !saves;

    return (
      <li key={task.id} className="tasks__item">
        <label className="tasks__label">
          <input
            type="checkbox"
            checked={on}
            disabled={!task.implemented}
            onChange={(e) => toggle(kind, task.id, e.target.checked)}
          />
          <span className={task.implemented ? "" : "muted"}>
            {labels[task.id] ?? task.id}
          </span>
        </label>
        {!task.implemented && <span className="tasks__note">henüz yok</span>}
        {orphaned && (
          <span className="tasks__note tasks__note--warn">
            "Dosyaya kaydet" olmadan çalışmaz
          </span>
        )}
      </li>
    );
  };

  return (
    <section className="card">
      <h2 className="card__title">Görev zinciri</h2>
      <p className="card__hint">
        Yakalama sonrası çalışacak adımlar. Sıra listedeki sıradır ve
        değiştirilemez — kaydetmek dosya yolunu kopyalamadan, yüklemek dosyayı
        silmeden önce gelmek zorunda.
      </p>

      <label className="tasks__target">
        <span>Hangi workflow</span>
        <select
          className="input"
          value={target ?? ""}
          onChange={(e) => setTarget(e.target.value || null)}
        >
          <option value="">Varsayılan (tüm workflow'lar)</option>
          {workflows.map((workflow) => (
            <option key={workflow.id} value={workflow.id}>
              {workflow.name}
            </option>
          ))}
        </select>
      </label>

      <h3 className="tasks__heading">Yakalama sonrası</h3>
      <ol className="tasks">
        {capture.map((task) => row(task, "capture", CAPTURE_LABELS, enabledCapture))}
      </ol>

      <h3 className="tasks__heading">Yükleme sonrası</h3>
      <ol className="tasks">
        {upload.map((task) => row(task, "upload", UPLOAD_LABELS, enabledUpload))}
      </ol>

      <p className="card__hint">
        Soluk görünenler henüz uygulanmadı. Seçilebilir bırakıp hiçbir şey
        yapmamaları, çalışıp etkisiz kalmakla karıştırılırdı.
      </p>

      {error && (
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      )}
    </section>
  );
}
