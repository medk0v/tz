import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { AgentTestTrace } from "./AgentTestTrace";

describe("agent test trace", () => {
  afterEach(cleanup);

  it("shows ordered article, API, and model events with recorded virtual time and HTTP failures", async () => {
    render(<AgentTestTrace trace={[
      { step: 0, at: 0, call: { tool: "read_article", parameters: { article_id: "article-1", version: 4, offset: 0 } }, response: { title: "Правила возврата", content: "Возврат выполняется после проверки." } },
      { step: 1, at: 30, call: { tool: "http", parameters: { method: "GET", url: "https://example.test/status" } }, response: '{"ok":false,"error":"request_failed"}', api: { status_code: 503, body: '{"reason":"unavailable"}' } },
      { step: 1, at: 30, response_model: "actual-model", configured_model: "configured-model", request_model: "request-model" },
    ]} />);
    const rows = within(screen.getByRole("list", { name: "События трассировки" })).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(within(rows[0]).getByText("Чтение статьи")).toBeInTheDocument();
    expect(within(rows[0]).getByText("Шаг 1")).toBeInTheDocument();
    expect(within(rows[0]).getByText("0 сек. сценария")).toBeInTheDocument();
    expect(within(rows[1]).getByText("HTTP 503")).toBeInTheDocument();
    expect(within(rows[1]).getByText("Ошибка")).toBeInTheDocument();
    expect(screen.queryByText("Пройдено")).not.toBeInTheDocument();
    fireEvent.click(within(rows[0]).getByText("Чтение статьи"));
    expect(await within(rows[0]).findByText("Возврат выполняется после проверки.")).toBeInTheDocument();
    fireEvent.click(within(rows[1]).getByText("HTTP-запрос"));
    expect(await within(rows[1]).findByRole("region", { name: "Ответ HTTP" })).toHaveTextContent('"reason": "unavailable"');
    fireEvent.click(within(rows[2]).getByText("Ответ модели"));
    expect(await within(rows[2]).findByText("Настроена")).toBeInTheDocument();
    expect(within(rows[2]).getByText("configured-model")).toBeInTheDocument();
  });

  it("keeps full unknown entries and redaction intact behind a lazy disclosure", async () => {
    const unknown = { custom_event: "new-format", payload: { secret: "[REDACTED]", html: '<script>alert("test")</script>' } };
    render(<AgentTestTrace trace={[unknown, "legacy event"]} />);
    expect(document.querySelector("pre")).toBeNull();
    fireEvent.click(screen.getAllByText("Событие")[0]);
    fireEvent.click(await screen.findByText("Исходные данные события"));
    await waitFor(() => expect(screen.getByText(JSON.stringify(unknown, null, 2), { selector: "pre", normalizer: (value) => value })).toBeInTheDocument());
    expect(document.querySelector("script")).toBeNull();
    expect(screen.queryByText(/сек\. сценария/)).not.toBeInTheDocument();
  });

  it("distinguishes articles stored in the test from published knowledge articles", () => {
    render(<AgentTestTrace testArticleIds={["fixture-1"]} trace={[
      { call: { tool: "read_article", parameters: { article_id: "fixture-1" } }, response: { title: "Тестовый ответ", content: "Ответ из сценария" } },
      { call: { tool: "read_article", parameters: { article_id: "published-1" } }, response: { title: "Рабочая статья", content: "Рабочий ответ" } },
    ]} />);
    const rows = screen.getAllByRole("listitem");
    expect(within(rows[0]).getByText("Чтение статьи внутри теста")).toBeInTheDocument();
    expect(within(rows[1]).getByText("Чтение статьи")).toBeInTheDocument();
  });

  it("bounds article previews and preserves the complete fragment when expanded", async () => {
    const content = `${"Длинный текст. ".repeat(150)}Конец исходного фрагмента`;
    render(<AgentTestTrace trace={[{ call: { tool: "read_article", parameters: { article_id: "article-1" } }, response: { content } }]} />);
    fireEvent.click(screen.getByText("Чтение статьи"));
    const full = await screen.findByRole("button", { name: "Показать полностью" });
    const fragment = screen.getByRole("region", { name: "Прочитанный фрагмент" });
    expect(fragment).not.toHaveTextContent("Конец исходного фрагмента");
    fireEvent.click(full);
    expect(fragment).toHaveTextContent("Конец исходного фрагмента");
    expect(full).toHaveAttribute("aria-expanded", "true");
  });

  it("reports an empty trace without implying that a test passed", () => {
    render(<AgentTestTrace trace={[]} />);
    expect(screen.getByText("В этом запуске нет событий трассировки.")).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });
});
