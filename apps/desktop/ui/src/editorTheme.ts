import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { EditorView, type Extension } from "@uiw/react-codemirror";

import type { ResolvedAppearance } from "./appearance";

/* The SQL editor is the loudest surface in the app, so it uses the same neutral
   ramp and single accent as the rest of the chrome instead of a stock theme. */

type EditorPalette = {
  text: string;
  faint: string;
  gutter: string;
  cursor: string;
  selection: string;
  matchingBracket: string;
  keyword: string;
  string: string;
  number: string;
  identifier: string;
  operator: string;
  comment: string;
  invalid: string;
};

const DARK: EditorPalette = {
  text: "#e6e6ea",
  faint: "#71717f",
  gutter: "#4d4d59",
  cursor: "#4ad4e8",
  selection: "rgba(74, 212, 232, 0.22)",
  matchingBracket: "rgba(74, 212, 232, 0.28)",
  keyword: "#4ad4e8",
  string: "#8fd6a0",
  number: "#f0b866",
  identifier: "#e6e6ea",
  operator: "#a4a4b0",
  comment: "#6b6b78",
  invalid: "#f87171",
};

const LIGHT: EditorPalette = {
  text: "#1b1b20",
  faint: "#82828e",
  gutter: "#a3a3ad",
  cursor: "#0b7f95",
  selection: "rgba(11, 127, 149, 0.16)",
  matchingBracket: "rgba(11, 127, 149, 0.2)",
  keyword: "#0b6d80",
  string: "#1c7245",
  number: "#96610a",
  identifier: "#1b1b20",
  operator: "#5b5b66",
  comment: "#82828e",
  invalid: "#c0353b",
};

function highlightStyle(palette: EditorPalette): HighlightStyle {
  return HighlightStyle.define([
    { tag: [tags.keyword, tags.modifier, tags.operatorKeyword], color: palette.keyword, fontWeight: "600" },
    { tag: [tags.string, tags.special(tags.string)], color: palette.string },
    { tag: [tags.number, tags.bool, tags.null], color: palette.number },
    { tag: [tags.typeName, tags.className, tags.definition(tags.typeName)], color: palette.keyword },
    { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: palette.identifier, fontWeight: "600" },
    { tag: [tags.variableName, tags.propertyName, tags.attributeName], color: palette.identifier },
    { tag: [tags.operator, tags.punctuation, tags.separator, tags.bracket], color: palette.operator },
    { tag: [tags.comment, tags.lineComment, tags.blockComment], color: palette.comment, fontStyle: "italic" },
    { tag: tags.invalid, color: palette.invalid },
  ]);
}

function baseTheme(palette: EditorPalette, dark: boolean): Extension {
  return EditorView.theme({
    "&": {
      color: palette.text,
      backgroundColor: "transparent",
    },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: palette.gutter,
      border: "none",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      minWidth: "34px",
      padding: "0 10px 0 12px",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: palette.cursor,
      borderLeftWidth: "2px",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: palette.selection,
    },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: palette.matchingBracket,
      outline: "none",
    },
    ".cm-placeholder": {
      color: palette.faint,
    },
    ".cm-tooltip": {
      border: "none",
      backgroundColor: dark ? "#1d1d24" : "#ffffff",
      color: palette.text,
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: palette.selection,
      color: palette.text,
    },
  }, { dark });
}

const themes: Record<ResolvedAppearance, Extension[]> = {
  dark: [baseTheme(DARK, true), syntaxHighlighting(highlightStyle(DARK))],
  light: [baseTheme(LIGHT, false), syntaxHighlighting(highlightStyle(LIGHT))],
};

export function editorTheme(appearance: ResolvedAppearance): Extension[] {
  return themes[appearance];
}
