import { useState } from "react";

import type { Target as Destination } from "./contracts";
import type { Phase, SessionView } from "./source";

const paths = {
  mic: "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3ZM5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8",
  arrow: "M5 12h14M13 6l6 6-6 6",
  copy: "M9 9h12v12H9zM15 5V3H3v12h2",
  play: "m8 4 12 8-12 8Z",
  pause: "M8 5v14M16 5v14",
  check: "m5 12 4 4L19 6",
  search: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14Zm5 12 6 6",
  edit: "m15 4 5 5M4 20l5-1L21 7l-5-5L4 14Z",
  retry: "M3 11a9 9 0 1 1 2 7M3 4v7h7",
  close: "m6 6 12 12M6 18 18 6",
  trash: "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6",
} as const;

export const Icon = ({ name }: { name: keyof typeof paths }) => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.6"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d={paths[name]} />
  </svg>
);

export const LevelMeter = ({
  level,
  bars = 16,
  className = "",
}: {
  level: number | null;
  bars?: number;
  className?: string;
}) => {
  const value = Math.max(0, Math.min(1, level ?? 0));
  const [shown, setShown] = useState({ input: level, value });

  if (shown.input !== level) {
    setShown({
      input: level,
      value: level === null ? 0 : Math.max(value, shown.value - 0.08),
    });
  }

  return (
    <span
      className={`meter ${className}`.trim()}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- the lit bars are children, which a <meter> element can't render
      role="meter"
      aria-label="Microphone level"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value * 100)}
      aria-valuetext={
        level === null ? "Waiting for microphone" : `${Math.round(value * 100)} percent`
      }
    >
      {Array.from({ length: bars }, (_, index) => (
        <span key={index} data-lit={shown.value > index / bars} />
      ))}
    </span>
  );
};

const phaseLabels: Record<Phase, string> = {
  recording: "Listening",
  finishing: "Finishing",
  saving: "Saving",
  saved: "Saved",
  reviewing: "Ready to insert",
  pasting: "Inserting",
  pasted: "Pasted",
  failed: "Needs attention",
};

export const sessionLabel = (session: SessionView) =>
  session.phase === "recording" && session.level === null
    ? "Waiting for microphone"
    : phaseLabels[session.phase];

export const Status = ({
  session,
  label,
  children,
}: {
  session?: SessionView;
  label?: string;
  children?: React.ReactNode;
}) => {
  const text = label ?? (session ? sessionLabel(session) : "");
  const live = session?.phase === "recording" && session.level !== null;

  return (
    <output className="status" key={text} data-live={live}>
      <span className="dot" />
      {text}
      {children}
    </output>
  );
};

export const targetName = (...targets: (Destination | null | undefined)[]) =>
  targets.flatMap((target) => (target ? [target.name, target.app_id] : [])).find(Boolean);

export const Target = ({ name }: { name: string | undefined }) =>
  name === undefined ? null : (
    <span className="target">
      <Icon name="arrow" />
      {name}
    </span>
  );

export const Transcript = ({ committed, partial }: { committed: string; partial: string }) => {
  const [shown, setShown] = useState({ committed, settled: committed.length });

  if (shown.committed !== committed) {
    setShown({
      committed,
      settled: committed.startsWith(shown.committed) ? shown.committed.length : committed.length,
    });
  }

  return (
    <>
      <span>{committed.slice(0, shown.settled)}</span>
      <span className="settle" key={committed.length}>
        {committed.slice(shown.settled)}
      </span>
      <span className="provisional">{partial}</span>
    </>
  );
};

const keepScrolledToEnd = (element: HTMLElement | null) => {
  const scrollToEnd = () => {
    if (element) {
      element.scrollTop = element.scrollHeight;
    }
  };

  const observer = new MutationObserver(scrollToEnd);

  if (element) {
    observer.observe(element, { childList: true, characterData: true, subtree: true });
  }

  scrollToEnd();

  return () => {
    observer.disconnect();
  };
};

export const TranscriptTail = ({ session }: { session: SessionView }) => (
  <div className="overlay-text" ref={keepScrolledToEnd}>
    <Transcript committed={session.committed} partial={session.partial} />
  </div>
);

export const modifier = /Mac|iPhone|iPad/u.test(navigator.platform) ? "⌘" : "Ctrl";

export const duration = (milliseconds: number) => {
  const seconds = Math.floor(milliseconds / 1000);

  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
};
