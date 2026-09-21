import { createContext } from "react";

/** Temporarily dismiss native modals while a public publication asks for its password again. */
export const NoteDatabaseSuspendedContext = createContext(false);
