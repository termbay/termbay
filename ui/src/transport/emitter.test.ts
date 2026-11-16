import { describe, expect, it } from "vitest";
import { Emitter } from "./emitter";

describe("Emitter", () => {
  it("drops events without listeners by default", () => {
    const e = new Emitter<number>();
    e.emit(1);
    const got: number[] = [];
    e.on((v) => got.push(v));
    e.emit(2);
    expect(got).toEqual([2]);
  });

  it("flushes buffered events to the first listener only", () => {
    const e = new Emitter<number>({ bufferUntilListener: true });
    e.emit(1);
    e.emit(2);
    const first: number[] = [];
    const second: number[] = [];
    e.on((v) => first.push(v));
    e.on((v) => second.push(v));
    e.emit(3);
    expect(first).toEqual([1, 2, 3]);
    expect(second).toEqual([3]);
  });

  it("stops delivering after dispose", () => {
    const e = new Emitter<number>();
    const got: number[] = [];
    const sub = e.on((v) => got.push(v));
    sub.dispose();
    e.emit(1);
    expect(got).toEqual([]);
  });
});
