import { useTranslation } from "react-i18next";
import { useDaemon } from "./store";

/** Avisa quando o app não fala com o daemon. Conectado, não mostra nada. */
export function ConnectionNotice() {
  const { t } = useTranslation();
  const connection = useDaemon((state) => state.connection);
  switch (connection) {
    case "connecting":
      return <span className="text-caption">{t("daemon.connecting")}</span>;
    case "offline":
      return (
        <p role="alert" className="alert-text text-caption">
          {t("daemon.offline")}
        </p>
      );
    case "incompatible":
      return (
        <p role="alert" className="alert-text text-caption">
          {t("daemon.incompatible")}
        </p>
      );
    case "online":
      return null;
  }
}
