import type { Locale } from "../i18n";
import type { AiModelType, AiProfileVoice } from "../api";

export const emptyProfileVoice: AiProfileVoice = {
  speech_to_text_connection_id: null,
  speech_to_text_model: null,
  text_to_speech_connection_id: null,
  text_to_speech_model: null,
  text_to_speech_voice: null,
  voice_replies_enabled: false,
};

const text = {
  en: {
    modelType: "Model type",
    "modelType.chat": "Chat",
    "modelType.speech_to_text": "Speech recognition",
    "modelType.text_to_speech": "Speech synthesis",
    modelTypeHelp: "Speech connections use the OpenAI-compatible /audio/transcriptions and /audio/speech endpoints, for example Whisper or Qwen3-TTS behind an HTTPS address.",
    modelTypeLocked: "The type cannot be changed after the connection is created.",
    speechProviderKind: "Speech connections require OpenAI or an OpenAI-compatible API.",
    voiceTitle: "Voice",
    voiceHelp: "Customer voice messages from Telegram are transcribed and answered like text. Recognition works on this agent's Telegram channels while the agent is active.",
    speechToText: "Speech recognition",
    speechToTextNone: "Do not recognize voice messages",
    speechToTextModel: "Recognition model",
    textToSpeech: "Speech synthesis",
    textToSpeechNone: "No speech synthesis",
    textToSpeechModel: "Synthesis model",
    textToSpeechVoice: "Voice",
    defaultModel: "Connection default",
    voiceReplies: "Answer voice messages with voice",
    voiceRepliesHelp: "The reply is also sent to Telegram as a voice message; operators can listen to it in the conversation.",
    assignedConnection: "Assigned connection",
    noSpeechConnections: "Add a speech recognition or speech synthesis connection on the Connections tab first.",
    voiceMessage: "Voice message",
  },
  ru: {
    modelType: "Тип модели",
    "modelType.chat": "Текст",
    "modelType.speech_to_text": "Распознавание речи",
    "modelType.text_to_speech": "Синтез речи",
    modelTypeHelp: "Речевые подключения работают через OpenAI-совместимые эндпоинты /audio/transcriptions и /audio/speech, например Whisper или Qwen3-TTS по HTTPS-адресу.",
    modelTypeLocked: "Тип нельзя изменить после создания подключения.",
    speechProviderKind: "Для речи нужен OpenAI или OpenAI-совместимый API.",
    voiceTitle: "Голос",
    voiceHelp: "Голосовые сообщения клиентов из Telegram распознаются в текст, и агент отвечает на них как на обычные. Распознавание работает в Telegram-каналах этого агента, пока агент активен.",
    speechToText: "Распознавание речи",
    speechToTextNone: "Не распознавать голосовые",
    speechToTextModel: "Модель распознавания",
    textToSpeech: "Синтез речи",
    textToSpeechNone: "Без синтеза речи",
    textToSpeechModel: "Модель синтеза",
    textToSpeechVoice: "Голос",
    defaultModel: "Модель подключения по умолчанию",
    voiceReplies: "Отвечать голосом на голосовые",
    voiceRepliesHelp: "Ответ дополнительно приходит в Telegram голосовым сообщением; оператор может прослушать его в диалоге.",
    assignedConnection: "Назначенное подключение",
    noSpeechConnections: "Сначала добавьте подключение с типом «Распознавание речи» или «Синтез речи» на вкладке подключений.",
    voiceMessage: "Голосовое сообщение",
  },
  ro: {
    modelType: "Tipul modelului",
    "modelType.chat": "Chat",
    "modelType.speech_to_text": "Recunoaștere vocală",
    "modelType.text_to_speech": "Sinteză vocală",
    modelTypeHelp: "Conexiunile vocale folosesc endpointurile compatibile OpenAI /audio/transcriptions și /audio/speech, de exemplu Whisper sau Qwen3-TTS la o adresă HTTPS.",
    modelTypeLocked: "Tipul nu poate fi schimbat după crearea conexiunii.",
    speechProviderKind: "Conexiunile vocale necesită OpenAI sau un API compatibil OpenAI.",
    voiceTitle: "Voce",
    voiceHelp: "Mesajele vocale ale clienților din Telegram sunt transcrise și primesc răspuns ca textul. Recunoașterea funcționează pe canalele Telegram ale acestui agent cât timp agentul este activ.",
    speechToText: "Recunoaștere vocală",
    speechToTextNone: "Nu recunoaște mesajele vocale",
    speechToTextModel: "Model de recunoaștere",
    textToSpeech: "Sinteză vocală",
    textToSpeechNone: "Fără sinteză vocală",
    textToSpeechModel: "Model de sinteză",
    textToSpeechVoice: "Voce",
    defaultModel: "Modelul implicit al conexiunii",
    voiceReplies: "Răspunde vocal la mesajele vocale",
    voiceRepliesHelp: "Răspunsul este trimis și ca mesaj vocal în Telegram; operatorii îl pot asculta în conversație.",
    assignedConnection: "Conexiune atribuită",
    noSpeechConnections: "Adăugați mai întâi o conexiune de recunoaștere sau sinteză vocală în fila Conexiuni.",
    voiceMessage: "Mesaj vocal",
  },
} satisfies Record<Locale, Record<string, string>>;

export type VoiceTextKey = keyof (typeof text)["en"];

export function voiceText(locale: Locale, key: VoiceTextKey): string {
  return text[locale][key];
}

export function modelTypeLabel(locale: Locale, modelType: AiModelType | undefined): string {
  return voiceText(locale, `modelType.${modelType ?? "chat"}`);
}
