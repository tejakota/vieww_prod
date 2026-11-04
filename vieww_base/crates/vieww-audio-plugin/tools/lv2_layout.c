/* Prints the LV2 C ABI's sizes and offsets from the official headers, to
 * compare with the layout test at the bottom of src/lv2/sys.rs: the numbers
 * there are this program's output on x86-64 Linux (and, by the ABI's own
 * fixed field types, the same everywhere LV2 runs).
 *
 * Usage: lv2_layout      (compiled by tools/check-plugins.sh)
 */
#include <stdio.h>
#include <stddef.h>
#include "lv2/core/lv2.h"
#include "lv2/urid/urid.h"
#include "lv2/atom/atom.h"
#include "lv2/state/state.h"
#define P(T) printf(#T " %zu\n", sizeof(T))
int main(void){
 P(LV2_Feature); P(LV2_URID_Map); P(LV2_URID_Unmap); P(LV2_Descriptor);
 printf("LV2_Descriptor.instantiate %zu\n", offsetof(LV2_Descriptor, instantiate));
 printf("LV2_Descriptor.run %zu\n", offsetof(LV2_Descriptor, run));
 printf("LV2_Descriptor.extension_data %zu\n", offsetof(LV2_Descriptor, extension_data));
 P(LV2_Atom); P(LV2_Atom_Float); P(LV2_Atom_Double); P(LV2_Atom_URID);
 P(LV2_Atom_Object_Body); P(LV2_Atom_Object); P(LV2_Atom_Property_Body);
 printf("LV2_Atom_Property_Body.value %zu\n", offsetof(LV2_Atom_Property_Body, value));
 P(LV2_Atom_Property); P(LV2_Atom_Sequence_Body); P(LV2_Atom_Sequence); P(LV2_Atom_Event);
 printf("LV2_Atom_Event.body %zu\n", offsetof(LV2_Atom_Event, body));
 P(LV2_State_Interface);
 return 0;
}
