# Graphical Abstraction Manager (GAM)

The GAM provides abstract UI primitives to other modules.

The goal is to have this module work in close conjunction with the
`graphics-server`, and all other modules would route abstract UI
requests through this module. 

## Structure
At a high level, you can think of the GAM as a firewall around the `graphics-server`. 
The `graphics-server` has no concept of what pixel belongs where; it's happy
to mutate a pixel that is anywhere within the physical hardware framebuffer.

Giving processes direct access to `graphics-server` means that a less trusted
program could draw into an OS-reserved area, thus presenting false information
to a user. The GAM solves this problem by dividing the screen into `Canvas` objects.

### Canvas

A `Canvas` is a minimal data structure that defines a physical region of the
screen that will display a set of primitives. `Canvas` structures are domiciled
in the GAM server, and are considered trusted by default, although there is
a flag that can be cleared to make everything within it untrusted.

Each `Canvas` has a 128-bit GUID. Application processes that wish to draw
something to the screen must refer to a `Canvas` by its 128-bit GUID; it is up
to the GAM to not share secure GUIDs with insecure processes. Thus the security
of a `Canvas` rests in the difficulty of guessing the 128-bit GUID, and also
in the system not leaking GUIDs.

Every GAM drawing object includes the GUID of the `Canvas` to which it should be drawn. 
Upon receiving a draw request, it validates that the GUID exists, and applies
any other relevant rules (for example, a higher security process can use the
GAM to prohibit all drawing of lower security processes by marking their
`Canvas` as not drawable). 

All GAM drawing objects specify pixel offsets from a `(0,0)` top-left coordinate
system. The GAM then handles translating these offsets from a virtual `(0,0)`
Canvas offset to a physical region of a screen through the `clip_rect` 
record within the `Canvas`. The coordinate space of the `clip_rect` is fixed
to the screen's coordinates, that is, `(0,0)` in the top left, X increasing
to the right, Y increasing down.

A `Canvas` also stores a `pan_offset`. The `pan_offset` is added to every 
coordinate inside the objects that refer to a `Canvas` and then the result
is clipped with `clip_rect`; this allows for easy implementation
of panning and scrolling. (Note: this feature is largely untested as of March 2021)

A `Canvas` has a `trust_level` associated with it. Higher numbers are more
trusted; 255 is the highest level of trust. Rules for drawing are as follows:

1. More trusted `Canvas` objects always render on top of lower trusted object
2. When a higher trusted `Canvas` object overlaps a lower trusted object,
   the lower trusted object is:
   - defaced using hatched lines with a random angle and spacing
   - further updates to the lower trusted object are disallowed.

Thus, a `Canvas` should *not* be thought of like a "window", as windows in
typical UIs are allowed to freely overlap and clipping is handled
by simply drawing over lower layers of content.

`Canvas` makes it strongly preferred to render trusted and untrusted data
side-by-side, rather than one on top of each other.

This policy is partially to help users be very
clear as to e.g. where a password box is vs. an image that looks a lot like
a password box; but the policy is also informed by the limitations of the underlying
hardware. In particular, the underlying memory LCD strongly
relies on "dirty bits" for good performance, and doing full-region redraws to
handle dirty rectangles on window movement is not an efficient use of dirty
bits. Reducing time spent redrawing partially obscured windows is also good
for performance and helps to simplify the code base, but these last two considerations
are quite minor compared to the primary concern of a "least confusing" UI when
it comes to differentiating between trustable and less trustable regions of the
screen.

Thus, the simple rule is: don't stack content types of different trust levels.
If you require content stacking, this can be done for content within a single
trust level by using multiple objects within a `Canvas`, as they have a `draw_order`
attribute and can handle content stacking; but between trust domains, it's both
a trust and complexity issue to allow for simultaneous stacking of trust domains
with live, full-content update of the underlying layers.

### TextView

A `TextView` object is a heavy data structure that contains both a `xous::String`
and metadata which guides the GAM on how to render the string. Please note
the philosophy of the GAM is to hide the exact details of how anything is
rendered to the calling application. This both allows users to have greater
control over customizing their interfaces, and also helps introduce a layer
of protection against phishing; however it also means that UX designers will not
be able to have exquisite control over the "look and feel" of their applications.

`TextView` objects are domiciled on the application process. The application
process is responsible for guiding the rough layout of where `TextView`s go
in a canvas. Once the object is finalized, the `TextView` objects
are then mutably lent to the GAM using an `rkyv` lend wrapper;
the calling thread then blocks until the GAM completes the rendering operation.

For layouts that need to adjust in height based on variable-length text strings,
the calling application can use the `bounds_hint`/`TextBounds` to help manage this. 
The bounds of a `TextView` can either be a fixed-sized rectangle, or a box that
grows up and out from a point plus a width. So, for example, a `TextView` could have
an anchor in the lower-right hand corner, plus a maximum width, and the height of the
box will be computed based as the text is rendered. The height of text can't be
known a-priori, because for example, emoji glyphs and hanzi characters will
have a different height than latin characters. A `dry_run` option is also
available for a `TextView` so one can simulate the rendering to determine the height
without paying the compuational price. 

One can think of a `TextView` as a text bubble, that can have rounded or square
corners, and its content string can be rendered with a selection of options
that specify the glyph style (*not* a font -- it's more of a hint than a specifier),
aligment, and the size of the text bubble. The text bubble can either be of a
fixed size (such that the string will show ellipses `...` if it overruns the
bubble), or of a dynamically growable size based on its content.

Thus, a typical "chat-style" app where text bubbles show a history of the chat
going from the most recent at the bottom of the screen to the oldest at the top,
would start by rendering variable-height text bubbles on the bottom, getting the
returned value of the rendered height, and setting the height of the next bubble
on top for rendering, and then rendering that. 

`TextView` can both be directly rendered to a `Canvas`, or managed by secondary
object such as a `Menu` or `List` to compose other UI elements.