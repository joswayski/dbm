export const PRODUCT_NAME = "Anybase";
export const SITE_URL = "https://anyba.se";
export const SITE_DESCRIPTION = "A fast database client for PostgreSQL, MySQL, and Redis.";

/** The repository keeps its old name until the GitHub rename (docs/anybase-migration.md). */
export const REPOSITORY = "joswayski/dbm";
export const REPO_URL = `https://github.com/${REPOSITORY}`;
export const AUTHOR_URL = "https://josevalerio.com";
export const CONTACT_EMAIL = "contact@josevalerio.com";

export const FEATURES = [
  "Browse databases, schemas, tables, and Redis keys, with filters, sorting, and CSV export.",
  "Edit rows safely: changes are staged, previewed, and saved together.",
  "SQL and Redis workbench tabs with per-connection history.",
  "Color-coded connections, with passwords kept in your OS keychain.",
] as const;

export const SCREENSHOTS = [
  {
    src: "/screenshots/change-preview.webp",
    alt: "Editing a table, with a before and after preview of a staged change",
    caption: "Preview every staged change before saving",
  },
  {
    src: "/screenshots/table-editing.webp",
    alt: "A table with staged edits and a staged delete",
    caption: "Staged edits and deletes",
  },
  {
    src: "/screenshots/query.webp",
    alt: "A SQL query with results and history",
    caption: "SQL workbench with history",
  },
  {
    src: "/screenshots/table-browsing.webp",
    alt: "Browsing a table with the row inspector open",
    caption: "Row inspector",
  },
  {
    src: "/screenshots/connection.webp",
    alt: "The connection settings dialog",
    caption: "Color-coded connections",
  },
] as const;
