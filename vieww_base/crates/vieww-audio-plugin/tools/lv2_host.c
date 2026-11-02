/* A minimal LV2 host in C, compiled against the official LV2 headers, that
 * loads a bundle, discovers the plugin's shape from its RDF (the TTL files
 * src/lv2/ttl.rs generates), and drives the library through the whole ABI
 * the vieww wrapper implements: lv2_descriptor, the urid:map feature,
 * atom:Sequence control ports forged with the reference atom forge (events
 * as MIDI atoms and patch:Set/patch:Get messages), the latency control
 * port, and the state extension. It is the proof that the Rust
 * transcription in src/lv2/ is the LV2 ABI rather than merely
 * self-consistent.
 *
 * The RDF scanner below understands the files ttl.rs generates — a fixed
 * shape this host is only ever pointed at — where a general host would use
 * a real Turtle parser; the shapes it reads (lv2:parameter lists, audio
 * port classes, atom:supports midi:MidiEvent) are exactly what the
 * generator writes and what the wrapper implements.
 *
 * Usage: lv2_host <bundle.lv2>      (exit status 0 = all passed)
 */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lv2/atom/atom.h"
#include "lv2/atom/forge.h"
#include "lv2/atom/util.h"
#include "lv2/core/lv2.h"
#include "lv2/midi/midi.h"
#include "lv2/patch/patch.h"
#include "lv2/state/state.h"
#include "lv2/urid/urid.h"

#ifdef _WIN32
#include <windows.h>
#define LOAD(p) ((void *)LoadLibraryA(p))
#define SYM(h, s) ((void *)GetProcAddress((HMODULE)(h), s))
#define CLOSE(h) FreeLibrary((HMODULE)(h))
#else
#include <dlfcn.h>
#define LOAD(p) dlopen(p, RTLD_NOW | RTLD_LOCAL)
#define SYM(h, s) dlsym(h, s)
#define CLOSE(h) dlclose(h)
#endif

static int failures = 0;
#define CHECK(cond, ...)                                                     \
   do {                                                                      \
      if (!(cond)) {                                                         \
         fprintf(stderr, "FAIL %s:%d: ", __FILE__, __LINE__);                \
         fprintf(stderr, __VA_ARGS__);                                       \
         fputc('\n', stderr);                                                \
         failures++;                                                         \
      }                                                                      \
   } while (0)

/* ---- urid:map ----------------------------------------------------------- */

#define MAX_URIS 512
static const char *uri_table[MAX_URIS];
static uint32_t n_uris = 0;

/* strdup is POSIX, not C11; this is the same thing. */
static const char *xstrdup(const char *s) {
   size_t n = strlen(s) + 1;
   char *d = malloc(n);
   if (d) memcpy(d, s, n);
   return d;
}

static LV2_URID map_uri(void *handle, const char *uri) {
   (void)handle;
   for (uint32_t i = 0; i < n_uris; i++)
      if (!strcmp(uri_table[i], uri)) return i + 1;
   if (n_uris < MAX_URIS) {
      uri_table[n_uris] = xstrdup(uri);
      return ++n_uris;
   }
   return 0;
}

static const char *unmap_uri(void *handle, LV2_URID urid) {
   (void)handle;
   return urid > 0 && urid <= n_uris ? uri_table[urid - 1] : NULL;
}

static LV2_URID_Map map = {NULL, map_uri};
static LV2_URID_Unmap unmap = {NULL, unmap_uri};
static const LV2_Feature map_feature = {LV2_URID__map, (void *)&map};
static const LV2_Feature unmap_feature = {LV2_URID__unmap, (void *)&unmap};
static const LV2_Feature *features[] = {&map_feature, &unmap_feature, NULL};

/* ---- bundle scanning (the fixed shape ttl.rs writes) -------------------- */

static char *read_file(const char *path, size_t *len) {
   FILE *f = fopen(path, "rb");
   if (!f) return NULL;
   fseek(f, 0, SEEK_END);
   long n = ftell(f);
   fseek(f, 0, SEEK_SET);
   char *buf = malloc((size_t)n + 1);
   if (!buf || fread(buf, 1, (size_t)n, f) != (size_t)n) {
      fclose(f);
      free(buf);
      return NULL;
   }
   fclose(f);
   buf[n] = 0;
   if (len) *len = (size_t)n;
   return buf;
}

/* The first `<...>` after `key` in `ttl`, into `out` (max n bytes). */
static int first_uriref(const char *ttl, const char *key, char *out, size_t n) {
   const char *p = strstr(ttl, key);
   if (!p) return 0;
   p += strlen(key);
   while (*p == ' ' || *p == '\t') p++;
   if (*p != '<') return 0;
   const char *e = ++p;
   while (*e && *e != '>') e++;
   if (*e != '>') return 0;
   size_t len = (size_t)(e - p);
   if (len >= n) return 0;
   memcpy(out, p, len);
   out[len] = 0;
   return 1;
}

/* The number after `key` at/after `b`. */
static double num_after(const char *b, const char *k) {
   const char *x = strstr(b, k);
   return x ? strtod(x + strlen(k), NULL) : 0.0;
}

typedef struct {
   char binary[512];          /* from manifest.ttl lv2:binary */
   char seealso[512];         /* from manifest.ttl rdfs:seeAlso */
   int in_ch, out_ch;         /* audio port classes in the plugin TTL */
   int midi;                  /* atom:supports midi:MidiEvent */
   char params[16][512];      /* lv2:parameter list */
   int n_params;
   double def, min, max;      /* first parameter's range */
} shape_t;

static void scan_bundle(const char *dir, shape_t *s) {
   char path[1024];
   memset(s, 0, sizeof *s);
   s->def = s->min = 0.0;
   s->max = 1.0;

   snprintf(path, sizeof path, "%s/manifest.ttl", dir);
   char *m = read_file(path, NULL);
   CHECK(m != NULL, "read %s", path);
   if (!m) return;
   CHECK(first_uriref(m, "lv2:binary", s->binary, sizeof s->binary),
         "manifest names lv2:binary");
   CHECK(first_uriref(m, "rdfs:seeAlso", s->seealso, sizeof s->seealso),
         "manifest names rdfs:seeAlso");
   free(m);

   snprintf(path, sizeof path, "%s/%s", dir, s->seealso);
   char *t = read_file(path, NULL);
   CHECK(t != NULL, "read %s", path);
   if (!t) return;

   const char *p = t;
   int in = 0, out = 0;
   while ((p = strstr(p, "lv2:InputPort , lv2:AudioPort")) != NULL) {
      in++;
      p++;
   }
   p = t;
   while ((p = strstr(p, "lv2:OutputPort , lv2:AudioPort")) != NULL) {
      out++;
      p++;
   }
   s->in_ch = in;
   s->out_ch = out;
   s->midi = strstr(t, "atom:supports midi:MidiEvent") != NULL;

   /* The one-line list ttl.rs writes: lv2:parameter <a> , <b> , ... . */
   const char *lp = strstr(t, "lv2:parameter ");
   if (lp) {
      const char *q = lp + strlen("lv2:parameter ");
      while (*q == ' ') q++;
      while (*q == '<' && s->n_params < 16) {
         const char *e = ++q;
         while (*e && *e != '>') e++;
         size_t len = (size_t)(e - q);
         if (*e != '>' || len >= sizeof s->params[0]) break;
         memcpy(s->params[s->n_params], q, len);
         s->params[s->n_params][len] = 0;
         s->n_params++;
         q = e + 1;
         while (*q == ' ' || *q == ',') q++;
      }
   }
   /* The first parameter's range: its subject block follows the list
    * (the URI's second occurrence). */
   if (s->n_params > 0) {
      const char *list = strstr(t, s->params[0]);
      const char *b = list ? strstr(list + 1, s->params[0]) : NULL;
      if (b) {
         s->def = num_after(b, "lv2:default ");
         s->min = num_after(b, "lv2:minimum ");
         s->max = num_after(b, "lv2:maximum ");
      }
   }
   free(t);
}

/* ---- state plumbing ------------------------------------------------------ */

typedef struct {
   uint32_t key;
   uint32_t type;
   uint8_t data[65536];
   size_t size;
} state_t;

static LV2_State_Status store(LV2_State_Handle handle, uint32_t key,
                              const void *value, size_t size, uint32_t type,
                              uint32_t flags) {
   (void)flags;
   state_t *s = (state_t *)handle;
   if (size > sizeof s->data) return LV2_STATE_ERR_NO_SPACE;
   s->key = key;
   s->type = type;
   s->size = size;
   memcpy(s->data, value, size);
   return LV2_STATE_SUCCESS;
}

static const void *retrieve(LV2_State_Handle handle, uint32_t key, size_t *size,
                            uint32_t *type, uint32_t *flags) {
   state_t *s = (state_t *)handle;
   if (key != s->key) return NULL; /* absent: a host reset */
   if (size) *size = s->size;
   if (type) *type = s->type;
   if (flags) *flags = 0;
   return s->data;
}

/* ---- the host ------------------------------------------------------------ */

static LV2_Atom_Forge forge;
#define ATOM_BUF 8192
#define MAX_FRAME 4096
static _Alignas(8) uint8_t in_atom[ATOM_BUF];
static _Alignas(8) uint8_t out_atom[ATOM_BUF];
static float audio_in[8][MAX_FRAME];
static float audio_out[8][MAX_FRAME];

/* Parse the patch:Set replies in the output sequence: fills value with the
 * value for `prop` when found; returns the number of Set messages. */
static int parse_sets(const LV2_Atom_Sequence *seq, LV2_URID prop, double *value) {
   LV2_URID urid_object = map_uri(NULL, LV2_ATOM__Object);
   LV2_URID patch_set = map_uri(NULL, LV2_PATCH__Set);
   LV2_URID patch_property = map_uri(NULL, LV2_PATCH__property);
   LV2_URID patch_value = map_uri(NULL, LV2_PATCH__value);
   LV2_URID atom_urid = map_uri(NULL, LV2_ATOM__URID);
   LV2_URID atom_float = map_uri(NULL, LV2_ATOM__Float);
   LV2_URID atom_double = map_uri(NULL, LV2_ATOM__Double);
   int n = 0;
   *value = NAN;
   LV2_ATOM_SEQUENCE_FOREACH (seq, ev) {
      const LV2_Atom *a = &ev->body;
      if (a->type != urid_object) continue;
      const LV2_Atom_Object *obj = (const LV2_Atom_Object *)a;
      if (obj->body.otype != patch_set) continue;
      n++;
      LV2_URID got_prop = 0;
      double got_value = NAN;
      LV2_ATOM_OBJECT_FOREACH (obj, p) {
         if (p->key == patch_property && p->value.type == atom_urid) {
            got_prop = ((const LV2_Atom_URID *)&p->value)->body;
         } else if (p->key == patch_value) {
            if (p->value.type == atom_float)
               got_value = ((const LV2_Atom_Float *)&p->value)->body;
            else if (p->value.type == atom_double)
               got_value = ((const LV2_Atom_Double *)&p->value)->body;
         }
      }
      if (got_prop == prop && !isnan(got_value)) *value = got_value;
   }
   return n;
}

static void connect_all(const LV2_Descriptor *d, LV2_Handle h, const shape_t *s,
                        float *latency) {
   d->connect_port(h, 0, in_atom);
   d->connect_port(h, 1, out_atom);
   d->connect_port(h, 2, latency);
   uint32_t port = 3;
   for (int c = 0; c < s->in_ch; c++) d->connect_port(h, port++, audio_in[c]);
   for (int c = 0; c < s->out_ch; c++) d->connect_port(h, port++, audio_out[c]);
}

/* Forge an empty sequence: the no-events block. */
static void empty_sequence(void) {
   LV2_Atom_Forge_Frame seqf;
   lv2_atom_forge_set_buffer(&forge, in_atom, sizeof in_atom);
   lv2_atom_forge_sequence_head(&forge, &seqf, 0);
   lv2_atom_forge_pop(&forge, &seqf);
}

static void test_plugin(const LV2_Descriptor *d, const char *bundle, const shape_t *s) {
   printf("  plugin <%s>\n", d->URI);
   CHECK(d->extension_data && d->extension_data(LV2_STATE__interface) != NULL,
         "state interface in extension_data");
   CHECK(d->extension_data("http://lv2plug.in/ns/ext/whatever") == NULL,
         "unknown extension_data is NULL");

   /* urid:map is required: instantiation without features must fail. */
   CHECK(d->instantiate(d, 48000.0, bundle, NULL) == NULL,
         "instantiation without features refused");

   LV2_Handle h = d->instantiate(d, 48000.0, bundle, features);
   CHECK(h != NULL, "instantiate");
   if (!h) return;
   float latency = -1.0f;
   connect_all(d, h, s, &latency);
   d->activate(h);

   const LV2_URID prop0 = map_uri(NULL, s->n_params > 0 ? s->params[0] : "");
   CHECK(s->n_params > 0, "at least one parameter in the TTL");
   CHECK(s->out_ch >= 1 && s->out_ch <= 8, "output channels");
   const double target = s->min + (s->max - s->min) * 0.25;
   printf("    %d in / %d out, %d parameters, midi:%s\n", s->in_ch, s->out_ch,
          s->n_params, s->midi ? "yes" : "no");
   printf("    first parameter default %g in [%g, %g], target %g\n", s->def,
          s->min, s->max, target);

   /* Block 1: a parameter change at frame 128 (plus a note for
    * instruments), forged with the reference atom forge. Unit 0 is the
    * default stamp type (audio frames) for run(), as atom.h says. The
    * effect input is DC: the gain's own glide is what must be visible,
    * not the input's phase. */
   for (int c = 0; c < 8; c++)
      for (int i = 0; i < MAX_FRAME; i++) {
         audio_in[c][i] = s->in_ch ? 0.5f : 0.0f;
         audio_out[c][i] = 0.0f;
      }
   {
      LV2_Atom_Forge_Frame seqf, objf;
      lv2_atom_forge_set_buffer(&forge, in_atom, sizeof in_atom);
      lv2_atom_forge_sequence_head(&forge, &seqf, 0);
      if (s->in_ch) { /* effect: automate at frame 128 */
         lv2_atom_forge_frame_time(&forge, 128);
         lv2_atom_forge_object(&forge, &objf, 0, map_uri(NULL, LV2_PATCH__Set));
         lv2_atom_forge_key(&forge, map_uri(NULL, LV2_PATCH__property));
         lv2_atom_forge_urid(&forge, prop0);
         lv2_atom_forge_key(&forge, map_uri(NULL, LV2_PATCH__value));
         lv2_atom_forge_float(&forge, (float)target);
         lv2_atom_forge_pop(&forge, &objf);
      } else { /* instrument: level at 0, note on at 100 */
         lv2_atom_forge_frame_time(&forge, 0);
         lv2_atom_forge_object(&forge, &objf, 0, map_uri(NULL, LV2_PATCH__Set));
         lv2_atom_forge_key(&forge, map_uri(NULL, LV2_PATCH__property));
         lv2_atom_forge_urid(&forge, prop0);
         lv2_atom_forge_key(&forge, map_uri(NULL, LV2_PATCH__value));
         lv2_atom_forge_float(&forge, (float)target);
         lv2_atom_forge_pop(&forge, &objf);
         lv2_atom_forge_frame_time(&forge, 100);
         const uint8_t msg[3] = {0x90, 69, 127};
         lv2_atom_forge_atom(&forge, sizeof msg, map_uri(NULL, LV2_MIDI__MidiEvent));
         lv2_atom_forge_write(&forge, msg, sizeof msg);
      }
      lv2_atom_forge_pop(&forge, &seqf);
   }
   d->run(h, 256);

   double energy = 0;
   int finite = 1;
   for (int c = 0; c < s->out_ch; c++)
      for (int i = 0; i < 256; i++) {
         energy += audio_out[c][i] * audio_out[c][i];
         finite &= isfinite(audio_out[c][i]);
      }
   CHECK(finite, "output finite");
   if (s->in_ch) {
      /* The change lands at 128 and the smoother glides down. */
      CHECK(fabsf(audio_out[0][127] - audio_in[0][127]) < 2e-3f,
            "pre-change output passes through: %g vs %g", audio_out[0][127],
            audio_in[0][127]);
      CHECK(fabsf(audio_out[0][128]) < fabsf(audio_out[0][127]),
            "change lands at frame 128");
      int mono = 1;
      for (int i = 129; i < 256; i++)
         if (fabsf(audio_out[0][i]) > fabsf(audio_out[0][128])) mono = 0;
      CHECK(mono, "monotonic glide after the change");
      CHECK(energy > 1e-6, "output has signal (energy %g)", energy);
   } else {
      CHECK(energy > 1e-4, "the note sounds (energy %g)", energy);
      for (int i = 0; i < 100; i++)
         CHECK(audio_out[0][i] == 0.0f, "silent before the note at frame 100");
   }
   printf("    latency port: %g samples\n", (double)latency);
   CHECK(latency >= 0.0f, "latency port written");

   /* Blocks of other sizes, including one frame. */
   empty_sequence();
   static const uint32_t sizes[] = {1, 7, 512, 64};
   for (unsigned k = 0; k < sizeof sizes / sizeof *sizes; k++) {
      memset(audio_out, 0, sizeof audio_out);
      d->run(h, sizes[k]);
      int ok = 1;
      for (int c = 0; c < s->out_ch; c++)
         for (uint32_t i = 0; i < sizes[k]; i++)
            ok &= isfinite(audio_out[c][i]);
      CHECK(ok, "block of %u finite", sizes[k]);
   }

   /* Instruments: note off, then the release decays to silence. */
   if (s->midi) {
      LV2_Atom_Forge_Frame seqf;
      lv2_atom_forge_set_buffer(&forge, in_atom, sizeof in_atom);
      lv2_atom_forge_sequence_head(&forge, &seqf, 0);
      lv2_atom_forge_frame_time(&forge, 0);
      const uint8_t off[3] = {0x80, 69, 64};
      lv2_atom_forge_atom(&forge, sizeof off, map_uri(NULL, LV2_MIDI__MidiEvent));
      lv2_atom_forge_write(&forge, off, sizeof off);
      lv2_atom_forge_pop(&forge, &seqf);
      d->run(h, 256);
      empty_sequence();
      for (int b = 0; b < 16; b++) d->run(h, MAX_FRAME);
      float peak = 0;
      for (int c = 0; c < s->out_ch; c++)
         for (int i = 0; i < MAX_FRAME; i++) {
            float v = fabsf(audio_out[c][i]);
            if (v > peak) peak = v;
         }
      CHECK(peak < 1e-3f, "released (peak %g)", peak);
   }

   /* patch:Get is answered with one patch:Set per parameter. */
   {
      LV2_Atom_Forge_Frame seqf, objf;
      lv2_atom_forge_set_buffer(&forge, in_atom, sizeof in_atom);
      lv2_atom_forge_sequence_head(&forge, &seqf, 0);
      lv2_atom_forge_frame_time(&forge, 0);
      lv2_atom_forge_object(&forge, &objf, 0, map_uri(NULL, LV2_PATCH__Get));
      lv2_atom_forge_pop(&forge, &objf);
      lv2_atom_forge_pop(&forge, &seqf);
   }
   d->run(h, 16);
   {
      double value = NAN;
      const LV2_Atom_Sequence *out = (const LV2_Atom_Sequence *)out_atom;
      int n = parse_sets(out, prop0, &value);
      CHECK(n == s->n_params, "Get answered with %d Sets (want %d)", n, s->n_params);
      CHECK(fabs(value - target) < 1e-5, "Set value %g == target %g", value, target);
   }

   /* State: save into a buffer, restore into a second instance. */
   const LV2_State_Interface *si =
      (const LV2_State_Interface *)d->extension_data(LV2_STATE__interface);
   CHECK(si != NULL && si->save != NULL && si->restore != NULL, "state functions");
   state_t st = {0, 0, {0}, 0};
   if (si && si->save) {
      CHECK(si->save(h, store, &st, 0, features) == LV2_STATE_SUCCESS, "state save");
      CHECK(st.size > 0, "state non-empty");

      LV2_Handle q = d->instantiate(d, 48000.0, bundle, features);
      CHECK(q != NULL, "second instance");
      if (q) {
         float lat2 = -1.0f;
         connect_all(d, q, s, &lat2);
         d->activate(q);
         CHECK(si->restore(q, retrieve, &st, 0, features) == LV2_STATE_SUCCESS,
               "state restore");
         /* The restored value reads back through patch:Get. */
         {
            LV2_Atom_Forge_Frame seqf, objf;
            lv2_atom_forge_set_buffer(&forge, in_atom, sizeof in_atom);
            lv2_atom_forge_sequence_head(&forge, &seqf, 0);
            lv2_atom_forge_frame_time(&forge, 0);
            lv2_atom_forge_object(&forge, &objf, 0, map_uri(NULL, LV2_PATCH__Get));
            lv2_atom_forge_pop(&forge, &objf);
            lv2_atom_forge_pop(&forge, &seqf);
         }
         d->run(q, 16);
         double value = NAN;
         int n = parse_sets((const LV2_Atom_Sequence *)out_atom, prop0, &value);
         CHECK(n == s->n_params, "restored Get answered (%d Sets)", n);
         CHECK(fabs(value - target) < 1e-5, "restored value %g == target %g", value,
               target);
         /* Garbage under the state key is refused, not half-applied. */
         state_t junk = st;
         memset(junk.data, 0xAB, junk.size > 32 ? 32 : junk.size);
         junk.size = 29; /* not a vieww state blob */
         CHECK(si->restore(q, retrieve, &junk, 0, features) != LV2_STATE_SUCCESS,
               "garbage state refused");
         /* An absent key is a host reset: success, nothing applied. */
         state_t absent = st;
         absent.key = st.key + 1;
         CHECK(si->restore(q, retrieve, &absent, 0, features) == LV2_STATE_SUCCESS,
               "absent state key is a reset");
         d->deactivate(q);
         d->cleanup(q);
      }
   }

   d->deactivate(h);
   d->cleanup(h);
}

int main(int argc, char **argv) {
   if (argc != 2) {
      fprintf(stderr, "usage: %s <bundle.lv2>\n", argv[0]);
      return 2;
   }
   shape_t s;
   scan_bundle(argv[1], &s);
   if (failures) {
      fprintf(stderr, "%d check(s) failed while scanning the bundle\n", failures);
      return 1;
   }
   printf("%s: binary %s, seeAlso %s\n", argv[1], s.binary, s.seealso);

   char path[1024];
   snprintf(path, sizeof path, "%s/%s", argv[1], s.binary);
   void *lib = LOAD(path);
   if (!lib) {
      fprintf(stderr, "cannot load %s\n", path);
      return 1;
   }
   const LV2_Descriptor *(*desc_fn)(uint32_t) =
      (const LV2_Descriptor *(*)(uint32_t))SYM(lib, "lv2_descriptor");
   CHECK(desc_fn != NULL, "lv2_descriptor exported");
   if (!desc_fn) return 1;

   lv2_atom_forge_init(&forge, &map);

   int n = 0;
   for (uint32_t i = 0; i < 256; i++) {
      const LV2_Descriptor *d = desc_fn(i);
      if (!d) break;
      CHECK(d->URI != NULL, "descriptor %u has a URI", i);
      if (d->URI) test_plugin(d, argv[1], &s);
      n++;
   }
   CHECK(n > 0, "at least one plugin");
   CHECK(desc_fn((uint32_t)n) == NULL, "table ends with NULL");

   CLOSE(lib);
   if (failures) {
      fprintf(stderr, "%d check(s) failed\n", failures);
      return 1;
   }
   printf("all LV2 checks passed\n");
   return 0;
}
