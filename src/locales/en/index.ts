import { app } from "./app";
import { errors } from "./errors";
import { glossary } from "./glossary";
import { history } from "./history";
import { overlay } from "./overlay";
import { permissions } from "./permissions";
import { settings } from "./settings";
import { styles } from "./styles";

/** Source catalog: every other language has the same shape (`Messages`). */
export const en = { app, errors, glossary, history, overlay, permissions, settings, styles };

export type Messages = typeof en;
