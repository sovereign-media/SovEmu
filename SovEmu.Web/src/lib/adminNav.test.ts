import { describe, expect, it } from "vitest";
import { ADMIN_NAV_GROUPS, adminDocumentTitle, findAdminNavGroup, findAdminNavItem } from "./adminNav";

describe("the admin menu", () => {
  it("names every page once", () => {
    const hrefs = ADMIN_NAV_GROUPS.flatMap((g) => g.items.map((i) => i.href));
    expect(new Set(hrefs).size).toBe(hrefs.length);
    for (const href of hrefs) expect(href).toMatch(/^\/admin\/([a-z-]+\/)?$/);
  });

  it("finds a page and its group with or without the trailing slash", () => {
    expect(findAdminNavItem("/admin/users/")?.label).toBe("Users");
    expect(findAdminNavItem("/admin/users")?.label).toBe("Users");
    expect(findAdminNavGroup("/admin/controllers/")?.id).toBe("users-devices");
    expect(findAdminNavGroup("/system/nes/")).toBeNull();
  });

  it("titles a page by its group", () => {
    expect(adminDocumentTitle("/admin/users/")).toBe("Users & Devices · Users");
    expect(adminDocumentTitle("/admin/")).toBe("Library");
    expect(adminDocumentTitle("/admin/unknown/")).toBe("Admin");
  });
});
