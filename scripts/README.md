Scripts to automatically generate code for src/pubmed/tags.rs

tags.txt was obtained by copy-pasting the HTML code for the table from
https://pubmed.ncbi.nlm.nih.gov/help/#pubmed-format, then a bash one-liner:

    grep -F '<td>' table.html | sed 's/.*<td>//' | sed 's/<\/.*//' > tags.txt

Enum variants are produced by running:

    python print_tag_comments.py < tags.txt

Helper functions were generated like this:

    cat tags.txt | python print_enum_variants.py | python print_to_tag.py
