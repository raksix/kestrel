import { useEffect, useState } from "react";
import { audioOptions, setAudio, type AudioOptions } from "../../lib/ipc";
import "./record.css";

/**
 * Which audio, if any, goes into a recording.
 *
 * Silence is the default and stays the default. A screen recording that
 * unexpectedly contains the room is a privacy problem, not a missing feature,
 * so nothing is picked until someone picks it.
 *
 * Loopback devices are listed first because "record what I hear" is the common
 * intent, but they are not auto-selected — the flag is a guess from the device
 * name, and choosing a microphone on a wrong guess is exactly the mistake this
 * panel exists to avoid.
 */
export default function AudioPicker() {
  const [options, setOptions] = useState<AudioOptions | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () =>
    audioOptions()
      .then((next) => {
        setOptions(next);
        setError(null);
      })
      .catch((e) => setError(String(e)));

  useEffect(() => {
    void load();
  }, []);

  const choose = (device: string | null, bitrate?: number) =>
    setAudio(device, bitrate)
      .then(load)
      .catch((e) => setError(String(e)));

  if (error) {
    return (
      <section className="card">
        <h2 className="card__title">Kayıt sesi</h2>
        <p className="status status--error" role="alert">
          <span className="dot" aria-hidden="true" />
          {error}
        </p>
      </section>
    );
  }

  if (!options) return null;

  const sorted = [...options.devices].sort(
    (a, b) => Number(b.likelyLoopback) - Number(a.likelyLoopback),
  );

  return (
    <section className="card">
      <h2 className="card__title">Kayıt sesi</h2>
      <p className="card__hint">
        Varsayılan sessiz. Ne kaydedileceğini sen seçmeden mikrofon açılmaz.
      </p>

      <label className="record__field">
        <span>Kaynak</span>
        <select
          className="input"
          value={options.selected ?? ""}
          onChange={(e) => void choose(e.target.value || null)}
        >
          <option value="">Ses kaydetme</option>
          {sorted.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
              {device.likelyLoopback ? " (sistem sesi olabilir)" : ""}
            </option>
          ))}
        </select>
      </label>

      {options.selected && (
        <label className="record__field">
          <span>Bit hızı</span>
          <input
            type="range"
            min={64}
            max={320}
            step={32}
            value={options.bitrateKbps}
            onChange={(e) => void choose(options.selected, Number(e.target.value))}
          />
          <span className="muted">{options.bitrateKbps} kbit/s</span>
        </label>
      )}

      {options.devices.length === 0 && (
        <p className="card__hint">
          ffmpeg hiç ses girişi göremedi. İzinler verilmiş mi diye bak, sonra
          bu paneli yeniden aç.
        </p>
      )}

      {options.systemAudioNote && (
        <p className="card__hint">{options.systemAudioNote}</p>
      )}

      <p className="card__hint">
        GIF kaydında ses yoktur — biçim taşımaz, bu yüzden seçim yok sayılır.
      </p>
    </section>
  );
}
