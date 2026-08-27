import type { ReactNode, SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & {
  size?: number;
};

function IconBase({ size = 16, children, ...props }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

export function IconPlus(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 3.25v9.5M3.25 8h9.5" />
    </IconBase>
  );
}

export function IconClose(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4.25 4.25l7.5 7.5M11.75 4.25l-7.5 7.5" />
    </IconBase>
  );
}

export function IconChevronLeft(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M10 3.5L5.5 8 10 12.5" />
    </IconBase>
  );
}

export function IconChevronRight(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M6 3.5L10.5 8 6 12.5" />
    </IconBase>
  );
}

export function IconChevronDown(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M3.5 6L8 10.5 12.5 6" />
    </IconBase>
  );
}

export function IconChevronUp(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M3.5 10L8 5.5 12.5 10" />
    </IconBase>
  );
}

export function IconMore(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="3.5" cy="8" r=".85" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r=".85" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="8" r=".85" fill="currentColor" stroke="none" />
    </IconBase>
  );
}

export function IconRefresh(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M13 8a5 5 0 1 1-1.4-3.4" />
      <path d="M13 3.25V6.5H9.75" />
    </IconBase>
  );
}

export function IconPencil(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M9.4 3.6l2.99 2.99M3.25 12.75l.82-3.54L10.46 3.8a1.2 1.2 0 0 1 1.7 0l.04.04a1.2 1.2 0 0 1 0 1.7L6.79 12.0l-3.54.75z" />
    </IconBase>
  );
}

export function IconPlay(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M5.5 3.75v8.5L13 8 5.5 3.75z" />
    </IconBase>
  );
}

export function IconTable(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="2.75" y="3.25" width="10.5" height="9.5" rx="1.5" />
      <path d="M2.75 6.5h10.5M6.5 6.5v6.25M9.5 6.5v6.25" />
    </IconBase>
  );
}

export function IconSchema(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M3.25 4.25h4.25v3H3.25zM8.5 8.75h4.25v3H8.5zM3.25 8.75h4.25v3H3.25z" />
      <path d="M5.4 7.25v1.5M10.6 8.75V7.25H7.5" />
    </IconBase>
  );
}

export function IconView(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M2.5 8s1.9-3.5 5.5-3.5S13.5 8 13.5 8s-1.9 3.5-5.5 3.5S2.5 8 2.5 8z" />
      <circle cx="8" cy="8" r="1.4" />
    </IconBase>
  );
}

export function IconCopy(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="5.25" y="5.25" width="7.5" height="7.5" rx="1.4" />
      <path d="M10.75 5.2V4.4A1.65 1.65 0 0 0 9.1 2.75H4.4A1.65 1.65 0 0 0 2.75 4.4v4.7A1.65 1.65 0 0 0 4.4 10.75h.8" />
    </IconBase>
  );
}

export function IconDownload(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 3.25v7.2M5.25 8.1L8 10.85 10.75 8.1" />
      <path d="M3.25 12.75h9.5" />
    </IconBase>
  );
}

export function IconFilter(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M2.75 3.75h10.5L9.4 8.2v3.3L6.6 13V8.2L2.75 3.75z" />
    </IconBase>
  );
}

export function IconHistory(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 4.25V8l2.3 1.6" />
      <path d="M4.05 5.4A5 5 0 1 1 3 8" />
      <path d="M3 3.75V6.4h2.6" />
    </IconBase>
  );
}

export function IconCheck(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M3.25 8.25l3 3.25 6.5-7" />
    </IconBase>
  );
}

export function IconAlert(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 6.2v2.4" />
      <circle cx="8" cy="11.15" r=".7" fill="currentColor" stroke="none" />
      <path d="M7.18 3.2L2.7 11.3A1.35 1.35 0 0 0 3.87 13.25h8.26a1.35 1.35 0 0 0 1.17-1.95L8.82 3.2a.95.95 0 0 0-1.64 0z" />
    </IconBase>
  );
}

export function IconFolder(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M2.75 5.1V11.4A1.35 1.35 0 0 0 4.1 12.75h7.8a1.35 1.35 0 0 0 1.35-1.35V6.6A1.35 1.35 0 0 0 11.9 5.25H8.1L6.7 3.75H4.1A1.35 1.35 0 0 0 2.75 5.1z" />
    </IconBase>
  );
}

export function IconPanelLeft(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="2.75" y="3.25" width="10.5" height="9.5" rx="1.5" />
      <path d="M6.25 3.25v9.5" />
    </IconBase>
  );
}

export function IconCollapse(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M10.75 3.5H13.5v2.75M10.75 12.5H13.5v-2.75M5.25 3.5H2.5v2.75M5.25 12.5H2.5v-2.75" />
      <path d="M13.15 3.85L10.4 6.6M13.15 12.15L10.4 9.4M2.85 3.85L5.6 6.6M2.85 12.15L5.6 9.4" />
    </IconBase>
  );
}

export function IconExpand(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M9.5 3.5H13.5v4M9.5 12.5H13.5v-4M6.5 3.5H2.5v4M6.5 12.5H2.5v-4" />
    </IconBase>
  );
}

export function IconArrowUp(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 12.5V3.5M4.75 6.75L8 3.5l3.25 3.25" />
    </IconBase>
  );
}

export function IconArrowDown(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 3.5v9M4.75 9.25L8 12.5l3.25-3.25" />
    </IconBase>
  );
}

export function IconSort(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M5 3.5v9M3.25 5.25L5 3.5l1.75 1.75M11 12.5v-9M9.25 10.75L11 12.5l1.75-1.75" />
    </IconBase>
  );
}

export function IconDatabase(props: IconProps) {
  return (
    <IconBase {...props}>
      <ellipse cx="8" cy="4.4" rx="4.75" ry="1.9" />
      <path d="M3.25 4.4v7.2c0 1.05 2.13 1.9 4.75 1.9s4.75-.85 4.75-1.9V4.4" />
      <path d="M3.25 8c0 1.05 2.13 1.9 4.75 1.9S12.75 9.05 12.75 8" />
    </IconBase>
  );
}

export function BrandMark({ size = 28 }: { size?: number }) {
  return (
    <svg
      className="brand-mark-glyph"
      width={size}
      height={size}
      viewBox="0 0 28 28"
      fill="none"
      aria-hidden="true"
    >
      <rect x="4" y="6" width="20" height="16" rx="3.5" stroke="currentColor" strokeWidth="1.5" />
      <path d="M4 11.25h20M11.25 11.25V22M16.75 11.25V22" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}
