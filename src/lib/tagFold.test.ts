// @author ygw
import { describe, expect, it } from "vitest";
import { filterTags, foldTags } from "./tagFold";

const tags = ["a", "b", "c", "d"].map((tag, i) => ({ tag, count: 10 - i }));

describe("标签折叠", () => {
  it("行内限量,其余折叠", () => {
    const r = foldTags(tags, null, 2);
    expect(r.visible.map((t) => t.tag)).toEqual(["a", "b"]);
    expect(r.rest.map((t) => t.tag)).toEqual(["c", "d"]);
  });
  it("已选标签不重复出现", () => {
    const r = foldTags(tags, "a", 2);
    expect(r.visible.map((t) => t.tag)).toEqual(["b", "c"]);
  });
  it("按关键词过滤并忽略大小写", () => {
    expect(filterTags([{ tag: "Rust", count: 1 }, { tag: "go", count: 1 }], "RU").length).toBe(1);
    expect(filterTags(tags, "  ").length).toBe(4);
  });
});
