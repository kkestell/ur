.PHONY: check check-docs format format-docs e2e install dev

check: check-docs

check-docs:
	dprint check

format: format-docs

format-docs:
	dprint fmt

e2e:
	python3 -m unittest discover -s tests

install:
	scripts/install

dev:
	scripts/dev '$(CURDIR)/tmux.conf'
