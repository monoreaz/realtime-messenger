// Scroll reveal: hide only elements that start below the fold, then reveal them on entry.
// Content above the fold never flickers, and without JS everything is simply visible.
const els = document.querySelectorAll<HTMLElement>("[data-reveal]");
const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

if (!reduced && "IntersectionObserver" in window) {
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        e.target.classList.remove("reveal-pending");
        io.unobserve(e.target);
      }
    },
    { rootMargin: "0px 0px -8% 0px" },
  );
  for (const el of els) {
    if (el.getBoundingClientRect().top > window.innerHeight) {
      el.classList.add("reveal-pending");
      io.observe(el);
    }
  }
}
