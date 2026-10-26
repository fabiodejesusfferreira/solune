import { Button, Chip, Logo } from "@solune/ui";
import { Upload } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useColorScheme } from "../App";
import { desktopSession, type Support } from "../daemon/api";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import { importFiles, useDropZone } from "./actions";

/** Primeiros passos: a biblioteca vazia vira um convite para importar. */
export function Onboarding() {
  const { t } = useTranslation();
  const scheme = useColorScheme();
  const dragging = useDropZone();
  const filter = t("library.filter");

  return (
    <main className="onboarding">
      <div className="onboarding__column">
        <header className="stack-4">
          <Logo variant="mark" on={scheme} size={64} />
          <h1 className="text-display onboarding__title">{t("onboarding.title")}</h1>
          <p className="text-body muted onboarding__intro">{t("onboarding.intro")}</p>
        </header>

        <div className="dropzone" data-dragging={dragging || undefined}>
          <Upload size={32} aria-hidden="true" className="muted" />
          <span className="text-body-strong">
            {dragging ? t("library.drop") : t("onboarding.drop")}
          </span>
          <span className="text-caption muted">{t("onboarding.formats")}</span>
          <div className="dropzone__actions">
            <Button variant="primary" onClick={() => void importFiles(filter)}>
              {t("onboarding.pick_file")}
            </Button>
            <Button onClick={() => void importFiles(filter, true)}>
              {t("onboarding.pick_folder")}
            </Button>
          </div>
        </div>
        <ConnectionNotice />

        <section className="stack-3" aria-labelledby="steps-title">
          <h2 id="steps-title" className="text-heading">
            {t("onboarding.steps")}
          </h2>
          <ol className="steps">
            <Step n={1} title={t("onboarding.step1")} text={t("onboarding.step1_help")} />
            <Step n={2} title={t("onboarding.step2")} text={t("onboarding.step2_help")} />
            <Step n={3} title={t("onboarding.step3")} text={t("onboarding.step3_help")} />
          </ol>
        </section>

        <System />
      </div>
    </main>
  );
}

function Step({ n, title, text }: { n: number; title: string; text: string }) {
  const { t } = useTranslation();
  return (
    <li className="steps__item">
      <span className="text-label sun-text">{t("onboarding.step", { n })}</span>
      <span className="text-body-strong">{title}</span>
      <span className="text-caption muted">{text}</span>
    </li>
  );
}

/** O sistema detectado e se o vídeo já funciona nele. */
function System() {
  const { t } = useTranslation();
  const [session, setSession] = useState<{ name: string; support: Support } | null>(null);
  useEffect(() => {
    desktopSession().then(setSession, () => {});
  }, []);
  if (!session) {
    return null;
  }
  const ready = session.support === "ready";
  const text = {
    ready: t("onboarding.system.ready_help"),
    needs_extension: t("onboarding.system.extension_help"),
    unsupported: t("onboarding.system.unsupported_help"),
  }[session.support];
  return (
    <section className="panel panel--padded stack-4" aria-labelledby="system-title">
      <h2 id="system-title" className="text-heading">
        {t("onboarding.system.title")}
      </h2>
      <div className="chips">
        <Chip>{session.name}</Chip>
        <Chip tone={ready ? "success" : "danger"} icon={ready ? "✓" : "!"}>
          {ready
            ? t("onboarding.system.ready")
            : session.support === "needs_extension"
              ? t("onboarding.system.needs_extension")
              : t("onboarding.system.unsupported")}
        </Chip>
      </div>
      <p className="text-body muted">{text}</p>
    </section>
  );
}
