import type { AiModelType, AiProfileVoice, AiProvider } from "../api";
import type { Locale } from "../i18n";
import { voiceText } from "./voice-i18n";

interface Props {
  locale: Locale;
  providers: AiProvider[];
  value: AiProfileVoice;
  onChange: (value: AiProfileVoice) => void;
}

function ConnectionOptions({ providers, modelType, selectedId, assignedLabel }: {
  providers: AiProvider[];
  modelType: AiModelType;
  selectedId: string | null | undefined;
  assignedLabel: string;
}) {
  const available = providers.filter((provider) => provider.model_type === modelType && provider.status === "active");
  return (
    <>
      {selectedId && !available.some((provider) => provider.id === selectedId) && <option value={selectedId}>{assignedLabel}</option>}
      {available.map((provider) => <option key={provider.id} value={provider.id}>{provider.name} · {provider.default_model}</option>)}
    </>
  );
}

/** Agent speech models: transcription of voice messages and optional spoken replies. */
export function AgentVoiceSettings({ locale, providers, value, onChange }: Props) {
  const text = (key: Parameters<typeof voiceText>[1]) => voiceText(locale, key);
  const hasSpeechConnections = providers.some((provider) => provider.model_type === "speech_to_text" || provider.model_type === "text_to_speech");
  const speechToText = value.speech_to_text_connection_id ?? null;
  const textToSpeech = value.text_to_speech_connection_id ?? null;
  return (
    <fieldset className="ai-assignment agent-voice-settings">
      <legend>{text("voiceTitle")}</legend>
      <p>{text("voiceHelp")}</p>
      {!hasSpeechConnections && !speechToText && !textToSpeech && <p className="agent-voice-empty">{text("noSpeechConnections")}</p>}
      <div className="ai-form-grid">
        <label>
          <span>{text("speechToText")}</span>
          <select value={speechToText ?? ""} onChange={(event) => onChange({
            ...value,
            speech_to_text_connection_id: event.target.value || null,
            speech_to_text_model: event.target.value ? value.speech_to_text_model ?? null : null,
          })}>
            <option value="">{text("speechToTextNone")}</option>
            <ConnectionOptions providers={providers} modelType="speech_to_text" selectedId={speechToText} assignedLabel={text("assignedConnection")} />
          </select>
        </label>
        <label>
          <span>{text("speechToTextModel")}</span>
          <input value={value.speech_to_text_model ?? ""} maxLength={200} disabled={!speechToText} placeholder={text("defaultModel")} autoCapitalize="none" spellCheck={false} onChange={(event) => onChange({ ...value, speech_to_text_model: event.target.value || null })} />
        </label>
        <label>
          <span>{text("textToSpeech")}</span>
          <select value={textToSpeech ?? ""} onChange={(event) => onChange(event.target.value
            ? { ...value, text_to_speech_connection_id: event.target.value }
            : { ...value, text_to_speech_connection_id: null, text_to_speech_model: null, text_to_speech_voice: null, voice_replies_enabled: false })}>
            <option value="">{text("textToSpeechNone")}</option>
            <ConnectionOptions providers={providers} modelType="text_to_speech" selectedId={textToSpeech} assignedLabel={text("assignedConnection")} />
          </select>
        </label>
        <label>
          <span>{text("textToSpeechModel")}</span>
          <input value={value.text_to_speech_model ?? ""} maxLength={200} disabled={!textToSpeech} placeholder={text("defaultModel")} autoCapitalize="none" spellCheck={false} onChange={(event) => onChange({ ...value, text_to_speech_model: event.target.value || null })} />
        </label>
        <label>
          <span>{text("textToSpeechVoice")}</span>
          <input value={value.text_to_speech_voice ?? ""} maxLength={100} disabled={!textToSpeech} placeholder="alloy" autoCapitalize="none" spellCheck={false} onChange={(event) => onChange({ ...value, text_to_speech_voice: event.target.value || null })} />
        </label>
      </div>
      <label>
        <input type="checkbox" checked={Boolean(value.voice_replies_enabled)} disabled={!textToSpeech} onChange={(event) => onChange({ ...value, voice_replies_enabled: event.target.checked })} />
        <span><strong>{text("voiceReplies")}</strong><small>{text("voiceRepliesHelp")}</small></span>
      </label>
    </fieldset>
  );
}
