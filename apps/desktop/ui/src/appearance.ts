import { create } from "zustand";

/* Appearance preference. "system" follows the OS on all three platforms, which
   is why the resolved value is stored separately from the preference. */

export type Appearance = "system" | "dark" | "light";
export type ResolvedAppearance = "dark" | "light";

const STORAGE_KEY = "dbm.appearance";
const DARK_QUERY = "(prefers-color-scheme: dark)";

function systemAppearance(): ResolvedAppearance {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return "dark";
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

function readPreference(): Appearance {
  if (typeof window === "undefined") return "system";
  const stored = window.localStorage.getItem(STORAGE_KEY);
  return stored === "dark" || stored === "light" || stored === "system" ? stored : "system";
}

function resolve(preference: Appearance): ResolvedAppearance {
  return preference === "system" ? systemAppearance() : preference;
}

function apply(resolved: ResolvedAppearance) {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.appearance = resolved;
}

type AppearanceStore = {
  preference: Appearance;
  resolved: ResolvedAppearance;
  setPreference: (preference: Appearance) => void;
  syncSystem: () => void;
};

const initialPreference = readPreference();
const initialResolved = resolve(initialPreference);
apply(initialResolved);

export const useAppearanceStore = create<AppearanceStore>((set, get) => ({
  preference: initialPreference,
  resolved: initialResolved,
  setPreference: (preference) => {
    const resolved = resolve(preference);
    if (typeof window !== "undefined") window.localStorage.setItem(STORAGE_KEY, preference);
    apply(resolved);
    set({ preference, resolved });
  },
  syncSystem: () => {
    if (get().preference !== "system") return;
    const resolved = systemAppearance();
    apply(resolved);
    set({ resolved });
  },
}));

export function watchSystemAppearance(): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => undefined;
  const media = window.matchMedia(DARK_QUERY);
  const listener = () => useAppearanceStore.getState().syncSystem();
  media.addEventListener?.("change", listener);
  return () => media.removeEventListener?.("change", listener);
}
