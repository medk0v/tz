import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Check, Star } from "lucide-react";
import {
  getPublicSupportRating,
  submitPublicSupportRating,
  type PublicSupportRating,
} from "./api";
import { normalizeWidgetLanguage, widgetMessages } from "./widget-i18n";

function accessTokenFromFragment(): string | null {
  const token = new URLSearchParams(window.location.hash.slice(1)).get("access")?.trim() || null;
  if (window.location.hash) {
    window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}`);
  }
  return token && token.length <= 128 ? token : null;
}

export function PublicRatingPage() {
  const [token] = useState(accessTokenFromFragment);
  const [presentation, setPresentation] = useState<PublicSupportRating | null>(null);
  const [selectedRating, setSelectedRating] = useState<number | null>(null);
  const [selectedReasons, setSelectedReasons] = useState<string[]>([]);
  const [comment, setComment] = useState("");
  const [loading, setLoading] = useState(Boolean(token));
  const [submitting, setSubmitting] = useState(false);
  const [complete, setComplete] = useState(false);
  const [error, setError] = useState(!token);
  const language = normalizeWidgetLanguage(presentation?.language) ?? "en";
  const text = useMemo(() => widgetMessages(language), [language]);

  useEffect(() => {
    if (!token) {
      return;
    }
    let active = true;
    getPublicSupportRating(token)
      .then((value) => {
        if (!active) return;
        setPresentation(value);
        setComplete(value.already_rated);
      })
      .catch(() => { if (active) setError(true); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [token]);

  useEffect(() => {
    document.documentElement.lang = language;
    document.title = language.startsWith("ru") ? "Оценка поддержки" : "Rate support";
  }, [language]);

  function chooseRating(rating: number) {
    setSelectedRating(rating);
    setSelectedReasons([]);
    setError(false);
  }

  function toggleReason(reason: string) {
    setSelectedReasons((current) => current.includes(reason)
      ? current.filter((value) => value !== reason)
      : [...current, reason]);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!token || !selectedRating || submitting) return;
    setSubmitting(true);
    setError(false);
    try {
      await submitPublicSupportRating(
        token,
        selectedRating,
        selectedReasons,
        comment.trim() || null,
      );
      setComplete(true);
    } catch {
      setError(true);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="public-rating-page">
      <section className="public-rating-card" aria-busy={loading}>
        {loading ? (
          <p className="public-rating-status" role="status">{language.startsWith("ru") ? "Загружаем форму…" : "Loading feedback form…"}</p>
        ) : error && !presentation ? (
          <div className="public-rating-message" role="alert"><strong>{language.startsWith("ru") ? "Ссылка недействительна" : "This link is unavailable"}</strong><p>{language.startsWith("ru") ? "Возможно, срок действия ссылки истёк." : "The invitation may have expired."}</p></div>
        ) : complete ? (
          <div className="public-rating-message public-rating-message--complete"><span><Check size={28} /></span><strong>{presentation?.rating_thanks || text.feedbackThanks}</strong></div>
        ) : presentation ? (
          <form onSubmit={(event) => void submit(event)}>
            <header><span>{language.startsWith("ru") ? "Оценка поддержки" : "Support feedback"}</span><h1>{presentation.rating_prompt}</h1></header>
            <div className="public-rating-stars" role="group" aria-label={presentation.rating_prompt}>
              {[1, 2, 3, 4, 5].map((rating) => (
                <button type="button" className={rating <= (selectedRating ?? 0) ? "is-active" : ""} aria-label={text.rate(rating)} aria-pressed={selectedRating === rating} key={rating} onClick={() => chooseRating(rating)}><Star aria-hidden="true" /></button>
              ))}
            </div>
            {selectedRating && (
              <div className="public-rating-fields">
                <fieldset><legend><strong>{text.ratingReasons}</strong><span>{text.ratingReasonsOptional}</span></legend><div>{text.ratingReasonOptions(selectedRating).map((reason) => <label className={selectedReasons.includes(reason.value) ? "is-selected" : ""} key={reason.value}><input type="checkbox" checked={selectedReasons.includes(reason.value)} onChange={() => toggleReason(reason.value)} /><span>{reason.label}</span></label>)}</div></fieldset>
                <label className="public-rating-comment"><span>{text.commentLabel}<small>{text.commentOptional}</small></span><textarea rows={4} maxLength={2_000} value={comment} placeholder={text.commentPlaceholder} onChange={(event) => setComment(event.currentTarget.value)} /></label>
                {error && <p className="public-rating-error" role="alert">{text.ratingError}</p>}
                <button className="public-rating-submit" type="submit" disabled={submitting}>{submitting ? text.submittingFeedback : text.submitFeedback}</button>
              </div>
            )}
          </form>
        ) : null}
      </section>
    </main>
  );
}
