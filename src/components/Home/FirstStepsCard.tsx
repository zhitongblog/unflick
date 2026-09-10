import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useIncognitoStore } from "../../stores/incognitoStore";
import { useStrings } from "../../i18n/utils";
import { SHOW_ONBOARDING_EVENT } from "../../lib/onboardingEvent";

interface Session {
  path: string;
}

/**
 * What the idle screen says when there is no history to say anything about.
 *
 * `ResumeCard` and `RecentFiles` both return null when empty, which on a
 * fresh install leaves the wordmark, one button, and a great deal of nothing
 * — a screen that answers no question anyone is asking. This is the third
 * sibling that fills that gap, and it is deliberately the only one of the
 * three that appears when the other two cannot.
 *
 * It asks the backend itself rather than being handed props by a parent that
 * fetched once. That is one extra IPC round trip on an idle screen, against
 * rewiring two components other work may be touching — and it keeps each
 * component's own incognito rule where that component can see it.
 */
export default function FirstStepsCard() {
  const [empty, setEmpty] = useState<boolean | null>(null);
  const incognito = useIncognitoStore((s) => s.enabled);
  const t = useStrings();

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      invoke<Session | null>("session_get").catch(() => null),
      invoke<unknown[]>("recent_list", { limit: 1 }).catch(() => []),
    ]).then(([session, recent]) => {
      if (cancelled) return;
      // Incognito blanks both of its siblings, so this card showing up then
      // is right: there genuinely is nothing to show.
      setEmpty(incognito || (!session && (!Array.isArray(recent) || recent.length === 0)));
    });
    return () => {
      cancelled = true;
    };
  }, [incognito]);

  // `null` while we do not know yet — rendering the "nothing here" copy and
  // then yanking it away as history arrives is worse than a beat of nothing.
  if (empty !== true) return null;

  return (
    <div className="idle-fade-in-delay-1 mt-8 w-full max-w-md text-center">
      <p className="text-[10px] font-semibold uppercase tracking-widest text-white/20">
        {t.home.firstStepsTitle}
      </p>
      <p className="mx-auto mt-2 max-w-sm text-[11.5px] leading-relaxed text-white/35">
        {t.home.firstStepsBody}
      </p>
      <p className="mt-3 text-[10.5px] text-white/20">
        {t.home.firstStepsHeadless}{" "}
        <button
          className="rounded px-1 text-white/35 underline decoration-white/20 underline-offset-2 transition-colors hover:text-white/70"
          onClick={() => window.dispatchEvent(new CustomEvent(SHOW_ONBOARDING_EVENT))}
        >
          {t.home.whatIsUnflick}
        </button>
      </p>
    </div>
  );
}
