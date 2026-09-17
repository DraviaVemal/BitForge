const DEFAULT_MESSAGE =
  "A build is running. BitBake processes one task at a time, so these are cached values — details refresh automatically once the build completes.";

export default function BuildActiveNote({ note }: { note?: string }) {
  return (
    <div className="section-note build-active">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <path d="M12 8v4l3 2" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <span>{note || DEFAULT_MESSAGE}</span>
    </div>
  );
}
