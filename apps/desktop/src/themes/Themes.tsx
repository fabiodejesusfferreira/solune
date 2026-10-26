import { Button, Chip } from "@solune/ui";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import type { Schedule, Theme } from "../daemon/protocol";
import { useDaemon } from "../daemon/store";
import { Thumb } from "../library/media";
import { type LibraryItem, slideFor, useLibrary } from "../library/store";

const toTime = (minutes: number) =>
  `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
const fromTime = (value: string) => {
  const [h, m] = value.split(":").map(Number);
  return (h ?? 0) * 60 + (m ?? 0);
};

export function Themes() {
  const { t } = useTranslation();
  const error = useDaemon((state) => state.error);
  return (
    <main className="page">
      <header className="stack-2">
        <h1 className="text-title">{t("themes.title")}</h1>
        <p className="text-body muted measure">{t("themes.intro")}</p>
      </header>
      <ConnectionNotice />
      {error ? (
        <p role="alert" className="alert-text text-caption">
          {t(`error.${error}`)}
        </p>
      ) : null}
      <div className="split split--reverse">
        <WhenToSwitch />
        <Preview />
      </div>
      <Pairs />
    </main>
  );
}

function WhenToSwitch() {
  const { t } = useTranslation();
  const { policy, connection, setSetting } = useDaemon();
  const lightId = useId();
  const darkId = useId();
  const schedule = policy?.settings.schedule;
  const disabled = !policy || connection !== "online";
  const set = (next: Schedule) => void setSetting("schedule", next);
  const times =
    schedule?.mode === "times"
      ? schedule
      : { mode: "times" as const, light_at: 420, dark_at: 1140 };
  const options: { mode: Schedule["mode"]; label: string; pick: Schedule }[] = [
    { mode: "system", label: t("themes.rule.system"), pick: { mode: "system" } },
    { mode: "times", label: t("themes.rule.times"), pick: times },
    {
      mode: "fixed",
      label: t("themes.rule.fixed"),
      pick: { mode: "fixed", theme: policy?.conditions.dark ? "dark" : "light" },
    },
  ];
  const help = {
    system: t("themes.rule.system_help"),
    times: t("themes.rule.times_help"),
    fixed: t("themes.rule.fixed_help"),
  };

  return (
    <section className="split__side panel panel--padded stack-4" aria-labelledby="when-title">
      <h2 id="when-title" className="text-heading">
        {t("themes.when")}
      </h2>
      <fieldset className="stack-2" aria-label={t("themes.when")}>
        {options.map((option) => (
          <Button
            key={option.mode}
            variant={schedule?.mode === option.mode ? "primary" : "secondary"}
            disabled={disabled}
            onClick={() => set(option.pick)}
          >
            {option.label}
          </Button>
        ))}
      </fieldset>
      {schedule ? <p className="text-caption muted">{help[schedule.mode]}</p> : null}
      {schedule?.mode === "times" ? (
        <div className="times">
          <label htmlFor={lightId} className="text-label muted">
            {t("themes.light_at")}
          </label>
          <input
            id={lightId}
            type="time"
            className="field"
            value={toTime(schedule.light_at)}
            disabled={disabled}
            onChange={(e) =>
              e.target.value && set({ ...schedule, light_at: fromTime(e.target.value) })
            }
          />
          <label htmlFor={darkId} className="text-label muted">
            {t("themes.dark_at")}
          </label>
          <input
            id={darkId}
            type="time"
            className="field"
            value={toTime(schedule.dark_at)}
            disabled={disabled}
            onChange={(e) =>
              e.target.value && set({ ...schedule, dark_at: fromTime(e.target.value) })
            }
          />
        </div>
      ) : null}
      {schedule?.mode === "fixed" ? (
        <fieldset className="chips" aria-label={t("themes.now")}>
          {(["light", "dark"] as const).map((theme) => (
            <Button
              key={theme}
              size="sm"
              variant={schedule.theme === theme ? "primary" : "secondary"}
              disabled={disabled}
              onClick={() => set({ mode: "fixed", theme })}
            >
              {theme === "light" ? t("themes.light") : t("themes.dark")}
            </Button>
          ))}
        </fieldset>
      ) : null}
    </section>
  );
}

/** O wallpaper em uso em cada tema: o par claro e escuro, se houver. */
function Preview() {
  const { t } = useTranslation();
  const status = useDaemon((state) => state.status);
  const items = useLibrary((state) => state.items);
  const [chosen, setChosen] = useState<Theme | null>(null);
  const theme = chosen ?? status?.theme ?? "light";
  const current = items.find((i) => i.path === status?.path);
  const slide = current ? slideFor(items, current.path) : null;
  const path = slide ? (theme === "dark" ? (slide.dark ?? slide.path) : slide.path) : null;
  const item = items.find((i) => i.path === path);
  return (
    <section className="split__main stack-4" aria-labelledby="preview-title">
      <div className="page__bar">
        <h2 id="preview-title" className="text-heading">
          {item ? t("themes.preview", { name: item.name }) : t("themes.preview_empty")}
        </h2>
        <fieldset className="chips" aria-label={t("themes.preview_theme")}>
          {(["light", "dark"] as const).map((option) => (
            <Button
              key={option}
              size="sm"
              variant={theme === option ? "primary" : "secondary"}
              onClick={() => setChosen(option)}
            >
              {option === "light" ? t("themes.light") : t("themes.dark")}
            </Button>
          ))}
        </fieldset>
      </div>
      <div className="preview">
        <Thumb item={item} />
      </div>
      <p className="text-caption muted">
        {status?.theme === "dark" ? t("themes.now_dark") : t("themes.now_light")}
      </p>
    </section>
  );
}

/** Os pares claro e escuro da biblioteca, e os que ainda faltam completar. */
function Pairs() {
  const { t } = useTranslation();
  const { items, setPair } = useLibrary();
  const seen = new Set<string>();
  const rows: { light?: LibraryItem | undefined; dark?: LibraryItem | undefined }[] = [];
  for (const item of items) {
    if (!item.theme || seen.has(item.path)) {
      continue;
    }
    const pair = items.find((i) => i.path === item.pair);
    seen.add(item.path);
    if (pair) {
      seen.add(pair.path);
    }
    rows.push(item.theme === "light" ? { light: item, dark: pair } : { light: pair, dark: item });
  }

  return (
    <section className="stack-4" aria-labelledby="pairs-title">
      <h2 id="pairs-title" className="text-heading">
        {t("themes.pairs")}
      </h2>
      {rows.length === 0 ? <p className="text-body muted">{t("themes.pairs_empty")}</p> : null}
      {rows.map((row) => {
        const owner = row.light ?? row.dark;
        if (!owner) {
          return null;
        }
        const missing: Theme | null = !row.light ? "light" : !row.dark ? "dark" : null;
        return (
          <div key={owner.path} className="pair-row panel">
            <div className="pair-row__name stack-2">
              <span className="text-body-strong">{owner.name}</span>
              <span>
                {missing ? (
                  <Chip tone="danger" icon="!">
                    {missing === "dark" ? t("themes.missing_dark") : t("themes.missing_light")}
                  </Chip>
                ) : (
                  <Chip tone="success" icon="✓">
                    {t("themes.complete")}
                  </Chip>
                )}
              </span>
            </div>
            {(["light", "dark"] as const).map((theme) => (
              <PairSlot
                key={theme}
                theme={theme}
                item={row[theme]}
                candidates={items.filter((i) => i.path !== owner.path)}
                onPick={(path) => setPair(owner.path, path)}
              />
            ))}
          </div>
        );
      })}
    </section>
  );
}

interface PairSlotProps {
  theme: Theme;
  item: LibraryItem | undefined;
  candidates: LibraryItem[];
  onPick: (path: string) => void;
}

function PairSlot({ theme, item, candidates, onPick }: PairSlotProps) {
  const { t } = useTranslation();
  const id = useId();
  const label = theme === "light" ? t("themes.light") : t("themes.dark");
  return (
    <div className="stack-1">
      <span className="text-label muted">{label}</span>
      {item ? (
        <span className="pair-row__art">
          <Thumb item={item} />
        </span>
      ) : (
        <span className="pair-row__art pair-row__art--empty">
          <label htmlFor={id} className="visually-hidden">
            {t("themes.pick", { theme: label })}
          </label>
          <select
            id={id}
            className="field field--select"
            value=""
            onChange={(e) => e.target.value && onPick(e.target.value)}
          >
            <option value="">{t("themes.choose")}</option>
            {candidates.map((c) => (
              <option key={c.path} value={c.path}>
                {c.name}
              </option>
            ))}
          </select>
        </span>
      )}
    </div>
  );
}
