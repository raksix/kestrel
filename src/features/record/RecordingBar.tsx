import { useEffect, useState } from "react";
import {
  cancelRecording,
  ffmpegStatus,
  onRecordingChanged,
  recordingStatus,
  setRecordingPaused,
  stopRecording,
  type FfmpegStatus,
  type RecordingStatus,
} from "../../lib/ipc";
import "./record.css";

const formatDuration = (seconds: number) => {
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${minutes}:${String(rest).padStart(2, "0")}`;
};

/**
 * The recording indicator.
 *
 * Only visible while recording, because a control that is dead most of the time
 * is worse than no control — and the tray already offers start.
 */
export default function RecordingBar() {
  const [status, setStatus] = useState<RecordingStatus | null>(null);
  const [ffmpeg, setFfmpeg] = useState<FfmpegStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    recordingStatus().then(setStatus).catch(() => undefined);
    ffmpegStatus().then(setFfmpeg).catch(() => undefined);

    const unlisten = onRecordingChanged(setStatus);
    return () => {
      unlisten.then((un) => un()).catch(() => undefined);
    };
  }, []);

  // The elapsed time lives in Rust; poll while running rather than counting
  // here, so a pause never drifts the two apart.
  useEffect(() => {
    if (!status?.active) return;
    const timer = window.setInterval(() => {
      recordingStatus().then(setStatus).catch(() => undefined);
    }, 500);
    return () => window.clearInterval(timer);
  }, [status?.active]);

  if (ffmpeg && !ffmpeg.available) {
    return (
      <aside className="recording recording--missing" role="status">
        <span>Ekran kaydı için ffmpeg gerekiyor</span>
        <code className="recording__hint">{ffmpeg.installHint}</code>
      </aside>
    );
  }

  if (!status?.active) return null;

  return (
    <aside className="recording" role="status" aria-live="polite">
      <span
        className={`recording__dot ${status.paused ? "recording__dot--paused" : ""}`}
        aria-hidden="true"
      />
      <span className="recording__time">{formatDuration(status.elapsed)}</span>

      <button
        type="button"
        className="button"
        onClick={async () => {
          try {
            setStatus(await setRecordingPaused(!status.paused));
          } catch (e) {
            setError(String(e));
          }
        }}
      >
        {status.paused ? "Devam" : "Duraklat"}
      </button>
      <button
        type="button"
        className="button button--primary"
        onClick={async () => {
          try {
            await stopRecording();
          } catch (e) {
            setError(String(e));
          }
        }}
      >
        Bitir
      </button>
      <button
        type="button"
        className="button"
        title="Kaydı at"
        onClick={() => void cancelRecording()}
      >
        İptal
      </button>

      {error && <span className="recording__error">{error}</span>}
    </aside>
  );
}
