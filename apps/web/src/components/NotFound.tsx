export function NotFound() {
  return (
    <main className="mx-auto flex min-h-dvh w-full max-w-2xl flex-col justify-center px-6 py-16">
      <p className="section-label">404</p>
      <h1 className="mt-3 text-2xl font-semibold tracking-tight text-ink-strong">This page doesn't exist.</h1>
      <p className="mt-4 text-sm text-ink-muted">
        <a href="/" className="text-link">
          Back to Anybase
        </a>
      </p>
    </main>
  );
}
