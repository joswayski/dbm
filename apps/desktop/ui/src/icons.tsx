import type { ReactNode } from "react";

/* Inline stroke icons on a 24px grid.
   Unicode glyphs (×, ⋯, ⌄, ▧) render at different weights and baselines across
   Apple Symbols, Segoe UI Symbol, and Noto, and sometimes as tofu, so every
   glyph in DBM is drawn here instead. */

export type IconProps = {
  size?: number;
  strokeWidth?: number;
  className?: string;
};

function Icon({ size = 14, strokeWidth = 1.75, className, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
      focusable="false"
    >{children}</svg>
  );
}

export const IconDatabase = (props: IconProps) => <Icon {...props}>
  <ellipse cx="12" cy="6" rx="7" ry="3" />
  <path d="M5 6v6c0 1.66 3.13 3 7 3s7-1.34 7-3V6" />
  <path d="M5 12v6c0 1.66 3.13 3 7 3s7-1.34 7-3v-6" />
</Icon>;

export const IconTable = (props: IconProps) => <Icon {...props}>
  <rect x="3" y="4" width="18" height="16" rx="2" />
  <path d="M3 9.5h18M9 9.5V20" />
</Icon>;

export const IconView = (props: IconProps) => <Icon {...props}>
  <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12S18 18.5 12 18.5 2.5 12 2.5 12Z" />
  <circle cx="12" cy="12" r="2.75" />
</Icon>;

export const IconSchema = (props: IconProps) => <Icon {...props}>
  <path d="m12 3 8.5 4.4L12 11.8 3.5 7.4 12 3Z" />
  <path d="m3.5 12.4 8.5 4.4 8.5-4.4" />
  <path d="m3.5 17 8.5 4.4 8.5-4.4" />
</Icon>;

export const IconCode = (props: IconProps) => <Icon {...props}>
  <path d="m9 6-5 6 5 6M15 6l5 6-5 6" />
</Icon>;

export const IconPlus = (props: IconProps) => <Icon {...props}>
  <path d="M12 5v14M5 12h14" />
</Icon>;

export const IconX = (props: IconProps) => <Icon {...props}>
  <path d="M18 6 6 18M6 6l12 12" />
</Icon>;

export const IconChevronRight = (props: IconProps) => <Icon {...props}>
  <path d="m9 6 6 6-6 6" />
</Icon>;

export const IconChevronLeft = (props: IconProps) => <Icon {...props}>
  <path d="m15 6-6 6 6 6" />
</Icon>;

export const IconChevronDown = (props: IconProps) => <Icon {...props}>
  <path d="m6 9 6 6 6-6" />
</Icon>;

export const IconChevronUp = (props: IconProps) => <Icon {...props}>
  <path d="m6 15 6-6 6 6" />
</Icon>;

export const IconChevronsUpDown = (props: IconProps) => <Icon {...props}>
  <path d="m7 14.5 5 5 5-5M7 9.5l5-5 5 5" />
</Icon>;

export const IconChevronsLeft = (props: IconProps) => <Icon {...props}>
  <path d="m11 6-6 6 6 6M19 6l-6 6 6 6" />
</Icon>;

export const IconChevronsRight = (props: IconProps) => <Icon {...props}>
  <path d="m13 6 6 6-6 6M5 6l6 6-6 6" />
</Icon>;

export const IconArrowUp = (props: IconProps) => <Icon {...props}>
  <path d="M12 19.5V5M6 11l6-6 6 6" />
</Icon>;

export const IconArrowDown = (props: IconProps) => <Icon {...props}>
  <path d="M12 4.5V19M6 13l6 6 6-6" />
</Icon>;

export const IconArrowRight = (props: IconProps) => <Icon {...props}>
  <path d="M4.5 12h15M13.5 6l6 6-6 6" />
</Icon>;

export const IconMore = (props: IconProps) => <Icon {...props} strokeWidth={0}>
  <circle cx="5" cy="12" r="1.6" fill="currentColor" />
  <circle cx="12" cy="12" r="1.6" fill="currentColor" />
  <circle cx="19" cy="12" r="1.6" fill="currentColor" />
</Icon>;

export const IconSearch = (props: IconProps) => <Icon {...props}>
  <circle cx="11" cy="11" r="6.5" />
  <path d="m20 20-3.7-3.7" />
</Icon>;

export const IconFilter = (props: IconProps) => <Icon {...props}>
  <path d="M4 5h16l-6.2 7.3v6.2L10.2 21v-8.7L4 5Z" />
</Icon>;

export const IconRefresh = (props: IconProps) => <Icon {...props}>
  <path d="M20.5 12a8.5 8.5 0 1 1-2.9-6.4" />
  <path d="M20.5 4v5.5H15" />
</Icon>;

export const IconDownload = (props: IconProps) => <Icon {...props}>
  <path d="M12 3.5v11M7.5 10 12 14.5 16.5 10" />
  <path d="M4.5 19.5h15" />
</Icon>;

export const IconCopy = (props: IconProps) => <Icon {...props}>
  <rect x="9" y="9" width="11.5" height="11.5" rx="2" />
  <path d="M15.5 5.9A2.4 2.4 0 0 0 13.1 3.5H5.9A2.4 2.4 0 0 0 3.5 5.9v7.2a2.4 2.4 0 0 0 2.4 2.4" />
</Icon>;

export const IconPlay = (props: IconProps) => <Icon {...props} strokeWidth={0}>
  <path d="M8 5.6a.8.8 0 0 1 1.2-.7l9 6.4a.8.8 0 0 1 0 1.4l-9 6.4A.8.8 0 0 1 8 18.4V5.6Z" fill="currentColor" />
</Icon>;

export const IconStop = (props: IconProps) => <Icon {...props} strokeWidth={0}>
  <rect x="6.5" y="6.5" width="11" height="11" rx="2" fill="currentColor" />
</Icon>;

export const IconHistory = (props: IconProps) => <Icon {...props}>
  <path d="M3.5 12a8.5 8.5 0 1 0 2.7-6.2" />
  <path d="M3.5 4.5V10H9" />
  <path d="M12 8.5V12l2.8 1.8" />
</Icon>;

export const IconSettings = (props: IconProps) => <Icon {...props}>
  <path d="M4 8h8.5M17.5 8H20M4 16h3.5M12.5 16H20" />
  <circle cx="15" cy="8" r="2.5" />
  <circle cx="10" cy="16" r="2.5" />
</Icon>;

export const IconSun = (props: IconProps) => <Icon {...props}>
  <circle cx="12" cy="12" r="3.75" />
  <path d="M12 2.5v2.2M12 19.3v2.2M4.4 4.4l1.6 1.6M18 18l1.6 1.6M2.5 12h2.2M19.3 12h2.2M4.4 19.6 6 18M18 6l1.6-1.6" />
</Icon>;

export const IconMoon = (props: IconProps) => <Icon {...props}>
  <path d="M20.5 14.8A8.6 8.6 0 0 1 9.2 3.5a8.7 8.7 0 1 0 11.3 11.3Z" />
</Icon>;

export const IconMonitor = (props: IconProps) => <Icon {...props}>
  <rect x="3" y="4" width="18" height="12.5" rx="2" />
  <path d="M9 20.5h6M12 16.5v4" />
</Icon>;

export const IconCheck = (props: IconProps) => <Icon {...props}>
  <path d="m5 12.5 4.5 4.5L19 7" />
</Icon>;

export const IconCheckCircle = (props: IconProps) => <Icon {...props}>
  <circle cx="12" cy="12" r="8.5" />
  <path d="m8.5 12 2.5 2.5 4.5-5" />
</Icon>;

export const IconAlertTriangle = (props: IconProps) => <Icon {...props}>
  <path d="M10.3 4.4 2.7 17.4A1.9 1.9 0 0 0 4.4 20.3h15.2a1.9 1.9 0 0 0 1.7-2.9L13.7 4.4a1.9 1.9 0 0 0-3.4 0Z" />
  <path d="M12 9.5v4M12 16.8h.01" />
</Icon>;

export const IconAlertCircle = (props: IconProps) => <Icon {...props}>
  <circle cx="12" cy="12" r="8.5" />
  <path d="M12 7.8v4.7M12 16.2h.01" />
</Icon>;

export const IconInfo = (props: IconProps) => <Icon {...props}>
  <circle cx="12" cy="12" r="8.5" />
  <path d="M12 11v5.2M12 7.8h.01" />
</Icon>;

export const IconLock = (props: IconProps) => <Icon {...props}>
  <rect x="4.5" y="10.5" width="15" height="9.5" rx="2" />
  <path d="M8.2 10.5V7.8a3.8 3.8 0 0 1 7.6 0v2.7" />
</Icon>;

export const IconShield = (props: IconProps) => <Icon {...props}>
  <path d="M12 3 5 5.4V11c0 4.4 3 8.3 7 9.6 4-1.3 7-5.2 7-9.6V5.4L12 3Z" />
</Icon>;

export const IconKey = (props: IconProps) => <Icon {...props}>
  <circle cx="8" cy="15.5" r="3.5" />
  <path d="m10.6 13 8.4-8.4M16.6 7l2.4 2.4" />
</Icon>;

export const IconTrash = (props: IconProps) => <Icon {...props}>
  <path d="M4.5 7h15M9.5 7V5.6A1.6 1.6 0 0 1 11.1 4h1.8a1.6 1.6 0 0 1 1.6 1.6V7" />
  <path d="m6.8 7 .7 12A2 2 0 0 0 9.5 21h5a2 2 0 0 0 2-1.9l.7-12" />
</Icon>;

export const IconPencil = (props: IconProps) => <Icon {...props}>
  <path d="M16.6 3.6a2.05 2.05 0 0 1 2.9 2.9L8.1 17.9 4 19l1.1-4.1L16.6 3.6Z" />
</Icon>;

export const IconPlug = (props: IconProps) => <Icon {...props}>
  <path d="M9.5 3v5M14.5 3v5" />
  <path d="M6.5 8h11v3.2a5.5 5.5 0 0 1-11 0V8Z" />
  <path d="M12 16.7V21" />
</Icon>;

export const IconPower = (props: IconProps) => <Icon {...props}>
  <path d="M12 3.5v8" />
  <path d="M17.7 6.8a8 8 0 1 1-11.4 0" />
</Icon>;

export const IconCommand = (props: IconProps) => <Icon {...props}>
  <path d="M18 3a3 3 0 0 0-3 3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 0 0 0-6Z" />
</Icon>;

export const IconPanelLeft = (props: IconProps) => <Icon {...props}>
  <rect x="3" y="4" width="18" height="16" rx="2" />
  <path d="M9.5 4v16" />
</Icon>;

export const IconFolderOpen = (props: IconProps) => <Icon {...props}>
  <path d="M3.5 9.5V6.5a2 2 0 0 1 2-2h3.2l2 2h5.8a2 2 0 0 1 2 2v1" />
  <path d="M3.9 9.5h16.4a1.4 1.4 0 0 1 1.36 1.75l-1.6 6.5a2 2 0 0 1-1.94 1.5H5.6a2 2 0 0 1-1.95-1.55L2.5 11.2A1.4 1.4 0 0 1 3.9 9.5Z" />
</Icon>;

export const IconRows = (props: IconProps) => <Icon {...props}>
  <rect x="3" y="4.5" width="18" height="15" rx="2" />
  <path d="M3 9.5h18M3 14.5h18" />
</Icon>;

export const IconColumns = (props: IconProps) => <Icon {...props}>
  <rect x="3" y="4.5" width="18" height="15" rx="2" />
  <path d="M9 4.5v15M15 4.5v15" />
</Icon>;

export const IconExternal = (props: IconProps) => <Icon {...props}>
  <path d="M14 4.5h5.5V10" />
  <path d="M19.5 4.5 11 13" />
  <path d="M18 14v4.5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-10a2 2 0 0 1 2-2h4.5" />
</Icon>;
