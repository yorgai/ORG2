import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import Button from "@src/components/Button";

import PageNotice from ".";

describe("PageNotice", () => {
  it("forwards an explicit live-region role", () => {
    const markup = renderToStaticMarkup(
      createElement(PageNotice, { type: "danger", role: "alert" }, "Failed")
    );

    expect(markup).toContain('role="alert"');
    expect(markup).toContain("Failed");
  });

  it("supports structured details without nesting block content in a span", () => {
    const markup = renderToStaticMarkup(
      createElement(
        PageNotice,
        { type: "danger" },
        createElement("pre", null, "First line\nSecond line")
      )
    );
    expect(markup).toContain("<pre>First line\nSecond line</pre></div>");
    expect(markup).not.toContain("</pre></span>");
  });

  it("renders one neutral surface for every type", () => {
    const types = ["success", "danger", "warning", "info"] as const;

    for (const type of types) {
      const markup = renderToStaticMarkup(
        createElement(PageNotice, { type }, "Body")
      );

      expect(markup).toContain("border-border-1");
      expect(markup).toContain("shadow-dropdown-soft");
      expect(markup).not.toContain("border-danger-3");
      expect(markup).not.toContain("border-warning-3");
      expect(markup).not.toContain("text-danger-6");
      expect(markup).not.toContain("text-warning-6");
    }
  });

  it("makes title, body and subtitle text selectable", () => {
    const markup = renderToStaticMarkup(
      createElement(
        PageNotice,
        { type: "danger", title: "Failed", subtitle: "Retry later" },
        "Stack trace"
      )
    );

    const selectableCount = markup.split("allow-select-deep").length - 1;
    expect(selectableCount).toBe(3);
    expect(markup.split("page-notice__text").length - 1).toBe(3);
    expect(markup).toContain('class="page-notice ');
  });

  it("lets the icon inherit the alert title color", () => {
    const markup = renderToStaticMarkup(
      createElement(PageNotice, { type: "danger", title: "Failed" }, "Details")
    );

    expect(markup).toContain('class="flex h-[14px] shrink-0 items-center"');
    expect(markup).not.toContain("items-center text-text-3");
  });

  it("groups the action and tertiary close button with a one-pixel gap", () => {
    const markup = renderToStaticMarkup(
      createElement(
        PageNotice,
        {
          title: "Upgrade required",
          action: createElement(Button, {
            variant: "tertiary",
            iconOnly: true,
            icon: "Refresh",
            "aria-label": "Refresh",
          }),
          onClose: () => undefined,
          closeAriaLabel: "Close",
        },
        "Upgrade the CLI"
      )
    );

    expect(markup).toContain("gap-px");
    expect(markup.indexOf('aria-label="Refresh"')).toBeLessThan(
      markup.indexOf('aria-label="Close"')
    );
    expect(markup.split("enabled:hover:bg-surface-hover").length - 1).toBe(2);
  });
});
