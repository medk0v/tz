import { fireEvent } from "@testing-library/react";

/** The hidden form-value carrier that belongs to a DateTimeField trigger (or any element inside the field). */
export function dateTimeFieldInput(element: HTMLElement): HTMLInputElement {
  const input = element.closest(".date-time-field")?.querySelector<HTMLInputElement>("input.date-time-field-native");
  if (!input) throw new Error("The element is not part of a DateTimeField.");
  return input;
}

/** Set the field's value the way a user selection would (fires the carrier's change event). */
export function changeDateTimeField(element: HTMLElement, value: string): void {
  fireEvent.change(dateTimeFieldInput(element), { target: { value } });
}
