import { Chip, Toggle } from "@solune/ui";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { AppMemory } from "../daemon/api";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import type { Conditions, SettingName } from "../daemon/protocol";
import { useDaemon } from "../daemon/store";
import { useAutostart } from "./autostart";
import { historySize, logicalCores, totalMemory, type UsageHistory } from "./usage";

export function Rules({ usage }: { usage: UsageHistory }) {
  const { t } = useTranslation();
  const error = useDaemon((state) => state.error);
  return (
    <main className="rules">
      <header className="rules__header">
        <h1 className="text-title">{t("rules.title")}</h1>
        <p className="rules__intro text-body">{t("rules.intro")}</p>
      </header>
      <ConnectionNotice />
      {error ? (
        <p role="alert" className="alert-text text-caption">
          {t(`error.${error}`)}
        </p>
      ) : null}
      <UsageNow history={usage} />
      <WhenToPause />
    </main>
  );
}

function UsageNow({ history }: { history: UsageHistory }) {
  const { t, i18n } = useTranslation();
  const status = useDaemon((state) => state.status);
  const { usage, memory, cpu } = history;
  const number = (value: number, digits: number) =>
    value.toLocaleString(i18n.language, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  const cores = logicalCores();
  const percent = (permille: number | null | undefined, divisor = 1) =>
    permille == null ? null : `${number(permille / 10 / divisor, 1)}%`;
  const megabytes = (bytes: number | null | undefined) => mebibytes(bytes, number);

  const metrics: { label: string; value: string | null }[] = [
    { label: t("rules.usage.cpu"), value: percent(usage?.cpu_permille, cores) },
    { label: t("rules.usage.gpu"), value: percent(usage?.gpu_permille) },
    { label: t("rules.usage.memory"), value: megabytes(totalMemory(memory)) },
    { label: t("rules.usage.gpu_memory"), value: megabytes(usage?.gpu_memory_bytes) },
    {
      label: t("rules.usage.fps"),
      value: usage?.fps == null ? null : t("rules.usage.fps_value", { value: usage.fps }),
    },
  ];

  const state = status?.state ?? "stopped";
  return (
    <section className="rules__card" aria-labelledby="usage-title">
      <div className="rules__card-header">
        <h2 id="usage-title" className="text-heading">
          {t("rules.usage.title")}
        </h2>
        <Chip tone={state === "playing" ? "sun" : "neutral"}>
          {state === "playing"
            ? t("player.state.playing")
            : state === "paused"
              ? t("player.state.paused")
              : t("player.state.stopped")}
        </Chip>
        {state === "paused" && status?.pause_reason ? (
          <span className="rules__muted text-caption">
            {t(`pause_reason.${status.pause_reason}`)}
          </span>
        ) : null}
        {usage?.released ? (
          <span className="rules__muted text-caption">{t("rules.usage.released")}</span>
        ) : null}
      </div>

      <dl className="rules__metrics">
        {metrics.map((metric) => (
          <div key={metric.label} className="rules__metric">
            <dt className="text-label">{metric.label}</dt>
            <dd className="rules__value">{metric.value ?? "—"}</dd>
          </div>
        ))}
      </dl>
      {usage && metrics.some((metric) => metric.value === null) ? (
        <p className="rules__muted text-caption">{t("rules.usage.unknown")}</p>
      ) : null}

      {memory ? <MemoryParts memory={memory} format={megabytes} /> : null}

      <CpuChart
        samples={cpu.map((permille) => permille / 10 / cores)}
        format={(value) => `${number(value, 1)}%`}
      />
    </section>
  );
}

/** Bytes em MiB, a mesma unidade do monitor do sistema. */
export function mebibytes(
  bytes: number | null | undefined,
  number: (value: number, digits: number) => string,
): string | null {
  return bytes == null ? null : `${number(bytes / 2 ** 20, 0)} MiB`;
}

/** O que entra no total de RAM, processo por processo, e como foi medido. */
function MemoryParts({
  memory,
  format,
}: {
  memory: AppMemory;
  format: (bytes: number | null) => string | null;
}) {
  const { t } = useTranslation();
  const parts = [
    {
      key: "daemon",
      label: t("rules.memory.daemon"),
      help: t("rules.memory.daemon_help"),
      bytes: memory.daemon,
    },
    {
      key: "app",
      label: t("rules.memory.app"),
      help: t("rules.memory.app_help"),
      bytes: memory.app,
    },
    {
      key: "webview",
      label: t("rules.memory.webview"),
      help: t("rules.memory.webview_help", { count: memory.webview_processes }),
      bytes: memory.webview,
    },
    // Só aparece enquanto existe, como o ffmpeg de uma miniatura.
    ...(memory.other_processes > 0
      ? [
          {
            key: "other",
            label: t("rules.memory.other"),
            help: t("rules.memory.other_help", { count: memory.other_processes }),
            bytes: memory.other,
          },
        ]
      : []),
  ];
  return (
    <div className="rules__parts">
      <h3 id="memory-title" className="text-label rules__muted">
        {t("rules.memory.title")}
      </h3>
      <dl aria-labelledby="memory-title">
        {parts.map((part) => (
          <div key={part.key} className="rules__part">
            <dt>
              <span className="text-body">{part.label}</span>
              <span className="rules__muted text-caption">{part.help}</span>
            </dt>
            <dd className="rules__data">{format(part.bytes) ?? "—"}</dd>
          </div>
        ))}
        <div className="rules__part rules__part--total">
          <dt className="text-body-strong">{t("rules.memory.total")}</dt>
          <dd className="rules__data">{format(totalMemory(memory)) ?? "—"}</dd>
        </div>
      </dl>
      <p className="rules__muted text-caption">
        {memory.metric === "pss" ? t("rules.memory.pss") : t("rules.memory.rss")}
      </p>
      <p className="rules__muted text-caption">{t("rules.memory.hidden")}</p>
    </div>
  );
}

/** Menor topo do gráfico, em %, para o ruído de uma CPU parada não parecer pico. */
const minChartTop = 2;
const chartWidth = 600;
const chartHeight = 80;

function CpuChart({ samples, format }: { samples: number[]; format: (value: number) => string }) {
  const { t } = useTranslation();
  const peak = Math.max(0, ...samples);
  const top = Math.max(minChartTop, peak * 1.25);
  // As leituras entram pela direita: a mais nova fica na borda.
  const step = chartWidth / (historySize - 1);
  const points = samples.map((value, i) => {
    const x = chartWidth - (samples.length - 1 - i) * step;
    const y = chartHeight - 1 - (value / top) * (chartHeight - 4);
    return [x, y] as const;
  });
  const last = points.at(-1);

  return (
    <figure className="rules__chart">
      <figcaption className="rules__chart-caption text-label">
        <span>{t("rules.usage.chart")}</span>
        {samples.length > 0 ? (
          <span className="rules__data">{t("rules.usage.peak", { value: format(peak) })}</span>
        ) : (
          <span>{t("rules.usage.measuring")}</span>
        )}
      </figcaption>
      <svg
        viewBox={`0 0 ${chartWidth} ${chartHeight}`}
        width="100%"
        height={chartHeight}
        preserveAspectRatio="none"
        role="img"
        aria-label={
          samples.length > 0
            ? t("rules.usage.chart_label", { value: format(peak) })
            : t("rules.usage.measuring")
        }
      >
        <line
          className="rules__chart-axis"
          x1="0"
          y1={chartHeight - 1}
          x2={chartWidth}
          y2={chartHeight - 1}
        />
        <line
          className="rules__chart-grid"
          x1="0"
          y1={chartHeight / 2}
          x2={chartWidth}
          y2={chartHeight / 2}
        />
        {points.length > 1 ? (
          <polyline
            className="rules__chart-line"
            points={points.map((p) => p.join(",")).join(" ")}
          />
        ) : null}
        {/* Um traço de comprimento zero com ponta redonda: continua um círculo
            quando o SVG estica na largura, o que um <circle> não faz. */}
        {last ? <path className="rules__chart-dot" d={`M${last[0]} ${last[1]}h0`} /> : null}
      </svg>
    </figure>
  );
}

interface RuleProps {
  label: string;
  help: string;
  /** Aviso extra, como "esta sessão não informa". */
  undetected?: string | undefined;
  checked: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}

function Rule({ label, help, undetected, checked, disabled, onChange }: RuleProps) {
  const id = useId();
  return (
    <div className="rules__rule">
      <Toggle
        checked={checked}
        disabled={disabled}
        onChange={onChange}
        label={label}
        describedBy={id}
      />
      <div id={id} className="rules__help text-caption">
        <p>{help}</p>
        {undetected ? <p>{undetected}</p> : null}
      </div>
    </div>
  );
}

function WhenToPause() {
  const { t } = useTranslation();
  const { policy, connection, setSetting } = useDaemon();
  const autostart = useAutostart();
  const conditions: Conditions | undefined = policy?.conditions;

  const rule = (
    name: Exclude<SettingName, "sound">,
    label: string,
    help: string,
    undetected?: string,
  ) => ({
    key: name,
    label,
    help,
    undetected,
    checked: policy?.settings[name] ?? false,
    disabled: !policy || connection !== "online",
    onChange: (value: boolean) => void setSetting(name, value),
  });

  const rules = [
    rule(
      "pause_on_fullscreen",
      t("rules.pause_on_fullscreen.label"),
      t("rules.pause_on_fullscreen.help"),
      conditions?.fullscreen === null ? t("rules.pause_on_fullscreen.undetected") : undefined,
    ),
    rule(
      "pause_on_battery",
      t("rules.pause_on_battery.label"),
      t("rules.pause_on_battery.help"),
      conditions?.on_battery === null ? t("rules.pause_on_battery.undetected") : undefined,
    ),
    rule(
      "pause_on_lock",
      t("rules.pause_on_lock.label"),
      t("rules.pause_on_lock.help"),
      conditions?.screen_locked === null ? t("rules.pause_on_lock.undetected") : undefined,
    ),
    rule(
      "release_when_paused",
      t("rules.release_when_paused.label"),
      t("rules.release_when_paused.help"),
    ),
    rule(
      "show_desktop_icons",
      t("rules.show_desktop_icons.label"),
      t("rules.show_desktop_icons.help"),
    ),
    {
      key: "autostart",
      label: t("rules.autostart.label"),
      help: t("rules.autostart.help"),
      undetected: autostart.failed ? t("rules.autostart.error") : undefined,
      checked: autostart.enabled ?? false,
      disabled: autostart.enabled === null,
      onChange: (value: boolean) => void autostart.set(value),
    },
  ];

  return (
    <section className="rules__list" aria-labelledby="when-title">
      <h2 id="when-title" className="text-heading">
        {t("rules.when")}
      </h2>
      {rules.map(({ key, ...r }) => (
        <Rule key={key} {...r} />
      ))}
    </section>
  );
}
