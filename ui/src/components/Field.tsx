/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Text fields that never overwrite what the user is typing.
//
// Servo inserts a typed character and fires the `input` event as a separate task. A React
// controlled input that re-renders in between (an IPC event arrives, a timer ticks) writes its
// stale state back into the field and the character is lost. These components keep the DOM in
// charge while typing and write `value` only when it changes from outside, so they behave like
// controlled inputs without the race. Use them for every text field in the UI.

import { useLayoutEffect, useRef, type InputHTMLAttributes, type Ref, type TextareaHTMLAttributes } from "react";

function useOwnValue<T extends HTMLInputElement | HTMLTextAreaElement>(value: string, ref: Ref<T> | undefined) {
  const node = useRef<T | null>(null);
  // The last value this field reported or was given; equal values are echoes of typing.
  const known = useRef(value);
  useLayoutEffect(() => {
    if (value === known.current) return;
    known.current = value;
    if (node.current && node.current.value !== value) node.current.value = value;
  }, [value]);
  const setRef = (el: T | null) => {
    node.current = el;
    if (typeof ref === "function") ref(el);
    else if (ref) (ref as { current: T | null }).current = el;
  };
  const typed = (v: string) => {
    known.current = v;
  };
  return { setRef, typed };
}

type InputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "defaultValue"> & { value: string; ref?: Ref<HTMLInputElement> };

export function Input({ value, onChange, ref, ...rest }: InputProps) {
  const { setRef, typed } = useOwnValue(value, ref);
  return (
    <input
      {...rest}
      ref={setRef}
      defaultValue={value}
      onChange={(e) => {
        typed(e.target.value);
        onChange?.(e);
      }}
    />
  );
}

type TextAreaProps = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "defaultValue"> & { value: string; ref?: Ref<HTMLTextAreaElement> };

export function TextArea({ value, onChange, ref, ...rest }: TextAreaProps) {
  const { setRef, typed } = useOwnValue(value, ref);
  return (
    <textarea
      {...rest}
      ref={setRef}
      defaultValue={value}
      onChange={(e) => {
        typed(e.target.value);
        onChange?.(e);
      }}
    />
  );
}
