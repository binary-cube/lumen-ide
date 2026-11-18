/* A small list of numbers. */
#include <stdio.h>
#include "list.h"

#define MAX_ITEMS 64
#define SQUARE(x) ((x) * (x))

typedef struct node {
    int value;
    struct node *next;
} node_t;

enum color { RED, GREEN };

static int count(const node_t *head, unsigned long limit) {
    int n = 0;
    for (const node_t *it = head; it != NULL && n < limit; it = it->next) {
        n++;
    }
    return n;
}

int main(void) {
    char letter = 'a';
    bool ok = true;
    printf("%d items\n", count(NULL, MAX_ITEMS));
    // Done.
    return SQUARE(2) + 0x1f;
}
