import { useEffect, useRef, type KeyboardEvent, type PointerEvent } from "react";

export type Screenshot = { src: string; alt: string; caption: string };

type ScreenshotViewerProps = {
  shots: readonly Screenshot[];
  /** The open screenshot, or null when the viewer is closed. */
  index: number | null;
  onIndexChange: (index: number) => void;
  onClose: () => void;
};

/** Horizontal travel, in CSS pixels, that turns a drag into a swipe. */
const SWIPE_DISTANCE = 48;

export function stepIndex(index: number, step: number, count: number): number {
  return (((index + step) % count) + count) % count;
}

export function ScreenshotViewer({ shots, index, onIndexChange, onClose }: ScreenshotViewerProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const swipeStartRef = useRef<number | null>(null);
  const open = index !== null;
  const position = index ?? 0;
  const shot = open ? shots[position] : undefined;

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    // Escape and dialog.close() both end here.
    dialog.addEventListener("close", onClose);
    return () => dialog.removeEventListener("close", onClose);
  }, [onClose]);

  useEffect(() => {
    if (!open) return;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = previousOverflow;
    };
  }, [open]);

  function step(direction: number) {
    if (index !== null) onIndexChange(stepIndex(index, direction, shots.length));
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDialogElement>) {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      step(-1);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      step(1);
    }
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    // Keep receiving the pointer when a swipe ends outside the image.
    event.currentTarget.setPointerCapture(event.pointerId);
    swipeStartRef.current = event.clientX;
  }

  function handlePointerUp(event: PointerEvent<HTMLDivElement>) {
    const start = swipeStartRef.current;
    swipeStartRef.current = null;
    if (start === null) return;
    const distance = event.clientX - start;
    if (Math.abs(distance) >= SWIPE_DISTANCE) step(distance < 0 ? 1 : -1);
  }

  return (
    <dialog
      ref={dialogRef}
      data-dismiss=""
      className="screenshot-viewer"
      aria-label="Screenshots"
      onKeyDown={handleKeyDown}
      // Clicks on the backdrop or the empty space around the image close the viewer.
      onClick={(event) => {
        if (event.target instanceof HTMLElement && event.target.dataset.dismiss !== undefined) {
          dialogRef.current?.close();
        }
      }}
    >
      {shot ? (
        <div className="screenshot-viewer-body" data-dismiss="">
          <button
            type="button"
            className="viewer-button absolute top-4 right-4"
            aria-label="Close"
            onClick={() => dialogRef.current?.close()}
          >
            <CloseIcon />
          </button>

          <figure className="flex w-full flex-col items-center" data-dismiss="">
            <div
              className="touch-pan-y select-none"
              onPointerDown={handlePointerDown}
              onPointerUp={handlePointerUp}
              onPointerCancel={() => {
                swipeStartRef.current = null;
              }}
            >
              <img
                key={shot.src}
                src={shot.src}
                alt={shot.alt}
                width={1600}
                height={1000}
                draggable={false}
                className="block h-auto max-h-[78dvh] w-auto max-w-full rounded-[10px] border border-border-strong"
              />
            </div>
            <figcaption className="mt-4 flex w-full max-w-xl items-center justify-between gap-4">
              <button type="button" className="viewer-button" aria-label="Previous screenshot" onClick={() => step(-1)}>
                <ChevronIcon direction="left" />
              </button>
              <span className="text-center text-sm text-ink-secondary" aria-live="polite">
                {shot.caption}
                <span className="ml-2 font-mono text-[0.6875rem] text-ink-faint">
                  {position + 1} / {shots.length}
                </span>
              </span>
              <button type="button" className="viewer-button" aria-label="Next screenshot" onClick={() => step(1)}>
                <ChevronIcon direction="right" />
              </button>
            </figcaption>
          </figure>
        </div>
      ) : null}
    </dialog>
  );
}

function ChevronIcon({ direction }: { direction: "left" | "right" }) {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <path
        d={direction === "left" ? "M10 3.5 5.5 8l4.5 4.5" : "M6 3.5 10.5 8 6 12.5"}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <path d="m4 4 8 8M12 4l-8 8" strokeLinecap="round" />
    </svg>
  );
}
