import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";

import { IconCheck, IconX } from "./icons";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  trailing,
  active,
  loading,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "md" | "sm";
  icon?: ReactNode;
  trailing?: ReactNode;
  active?: boolean;
  loading?: boolean;
}) {
  const classes = [
    "btn",
    `btn-${variant}`,
    size === "sm" ? "btn-sm" : "",
    active ? "btn-active" : "",
    className ?? "",
  ].filter(Boolean).join(" ");
  return (
    <button type="button" className={classes} {...rest}>
      {loading ? <span className="spinner" /> : icon}
      {children}
      {trailing}
    </button>
  );
}

export function IconButton({
  icon,
  label,
  tooltip,
  tooltipAlign,
  size = "md",
  variant = "ghost",
  className,
  ...rest
}: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  icon: ReactNode;
  label: string;
  tooltip?: string | false;
  tooltipAlign?: "start" | "center" | "end";
  size?: "md" | "sm";
  variant?: "ghost" | "bordered" | "danger";
}) {
  const hint = tooltip === false ? undefined : tooltip ?? label;
  const classes = [
    "icon-btn",
    size === "sm" ? "icon-btn-sm" : "",
    variant === "bordered" ? "icon-btn-bordered" : "",
    variant === "danger" ? "icon-btn-danger" : "",
    className ?? "",
  ].filter(Boolean).join(" ");
  return (
    <button
      type="button"
      className={classes}
      aria-label={label}
      data-tooltip={hint}
      data-tooltip-align={hint && tooltipAlign && tooltipAlign !== "center" ? tooltipAlign : undefined}
      {...rest}
    >{icon}</button>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <span className="kbd">{children}</span>;
}

export function Badge({
  tone = "neutral",
  icon,
  children,
  ...rest
}: {
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
  icon?: ReactNode;
  children: ReactNode;
  title?: string;
  className?: string;
}) {
  const { className, ...attrs } = rest;
  return (
    <span className={["badge", tone === "neutral" ? "" : `badge-${tone}`, className ?? ""].filter(Boolean).join(" ")} {...attrs}>
      {icon}
      {children}
    </span>
  );
}

export function Count({ children }: { children: ReactNode }) {
  return <span className="badge badge-count">{children}</span>;
}

export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: Array<{ value: T; label: string; icon?: ReactNode }>;
  value: T;
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className="segmented-option"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
        >
          {option.icon}
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Field({
  label,
  hint,
  full,
  htmlFor,
  children,
}: {
  label: string;
  hint?: string;
  full?: boolean;
  htmlFor?: string;
  children: ReactNode;
}) {
  const content = <>
    <span>{label}</span>
    {children}
    {hint ? <span className="field-hint">{hint}</span> : null}
  </>;
  const className = ["field", full ? "full" : ""].filter(Boolean).join(" ");
  if (htmlFor) {
    return <div className={className}>
      <label className="field-label" htmlFor={htmlFor}>{label}</label>
      {children}
      {hint ? <span className="field-hint">{hint}</span> : null}
    </div>;
  }
  return <label className={className}>{content}</label>;
}

export function TextField({
  label,
  hint,
  full,
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string; full?: boolean }) {
  return (
    <Field label={label} hint={hint} full={full}>
      <input className={["input", className ?? ""].filter(Boolean).join(" ")} {...rest} />
    </Field>
  );
}

export function SelectField({
  label,
  hint,
  full,
  className,
  children,
  ...rest
}: SelectHTMLAttributes<HTMLSelectElement> & { label: string; hint?: string; full?: boolean }) {
  return (
    <Field label={label} hint={hint} full={full}>
      <select className={["select", className ?? ""].filter(Boolean).join(" ")} {...rest}>{children}</select>
    </Field>
  );
}

export function CheckboxField({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="checkbox-field">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      <span>
        {label}
        {hint ? <small>{hint}</small> : null}
      </span>
    </label>
  );
}

export function MenuItem({
  icon,
  label,
  onClick,
  danger,
  checked,
  kbd,
  title,
  role = "menuitem",
}: {
  icon?: ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
  checked?: boolean;
  kbd?: string;
  title?: string;
  role?: string;
}) {
  return (
    <button
      type="button"
      role={role}
      className={["menu-item", danger ? "danger" : ""].filter(Boolean).join(" ")}
      onClick={onClick}
      title={title}
    >
      {icon}
      <span>{label}</span>
      {kbd ? <Kbd>{kbd}</Kbd> : null}
      {checked ? <IconCheck size={13} className="menu-check" /> : null}
    </button>
  );
}

export function Menu({
  align = "end",
  label,
  children,
}: {
  align?: "start" | "end";
  label?: string;
  children: ReactNode;
}) {
  return (
    <div className={["menu", align === "start" ? "menu-left" : ""].filter(Boolean).join(" ")} role="menu" aria-label={label}>
      {children}
    </div>
  );
}

export function MenuSeparator() {
  return <div className="menu-separator" role="separator" />;
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return <div className="menu-label">{children}</div>;
}

export function Sheet({
  title,
  titleId,
  eyebrow,
  onClose,
  children,
  footer,
}: {
  title: string;
  titleId: string;
  eyebrow?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div className="scrim" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div className="sheet" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="sheet-header">
          {eyebrow}
          <h2 id={titleId}>{title}</h2>
          <IconButton icon={<IconX size={15} />} label="Close" tooltip={false} onClick={onClose} />
        </div>
        <div className="sheet-body">{children}</div>
        {footer ? <div className="sheet-footer">{footer}</div> : null}
      </div>
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  description,
  actions,
}: {
  icon?: ReactNode;
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="empty-state">
      {icon ? <div className="empty-state-icon">{icon}</div> : null}
      <strong>{title}</strong>
      {description ? <p>{description}</p> : null}
      {actions ? <div className="empty-state-actions">{actions}</div> : null}
    </div>
  );
}

export function Notice({
  tone = "neutral",
  icon,
  children,
  actions,
  onDismiss,
  className,
  role,
}: {
  tone?: "neutral" | "info" | "success" | "warning" | "danger";
  icon?: ReactNode;
  children: ReactNode;
  actions?: ReactNode;
  onDismiss?: () => void;
  className?: string;
  role?: string;
}) {
  return (
    <div
      className={["notice", tone === "neutral" ? "" : `notice-${tone}`, className ?? ""].filter(Boolean).join(" ")}
      role={role}
    >
      {icon}
      <span>{children}</span>
      {actions ? <div className="notice-actions">{actions}</div> : null}
      {onDismiss ? <IconButton
        className="notice-dismiss"
        size="sm"
        icon={<IconX size={13} />}
        label="Dismiss message"
        tooltip={false}
        onClick={onDismiss}
      /> : null}
    </div>
  );
}
