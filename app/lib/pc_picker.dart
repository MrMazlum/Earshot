// The "Your PC" card: the list of machines that answered the sweep, and the address field for the
// ones that cannot answer it.
//
// In its own file so test/gen_screens.dart can photograph it with results in it. The sweep needs a
// real network, which a widget test does not have, so the state that matters most — two PCs found,
// one of them picked — would otherwise never be looked at before it shipped.

import 'package:flutter/material.dart';

import 'discover.dart';

/// Your PC: whoever answered the sweep, and an address field underneath for when nobody did.
///
/// The list is the normal path — one tap, no typing, because the phone asked the network rather
/// than asking the user. The field is not an "advanced" mode hidden behind a toggle: a PC on
/// another subnet or behind a VPN cannot be found by knocking, and that is common enough to be
/// visible rather than discoverable.
class PcPicker extends StatelessWidget {
  final List<Found> found;
  final bool searching;
  final Destination? selected;
  final bool enabled;
  final TextEditingController host;
  final TextEditingController port;
  final void Function(Destination) onPick;
  final VoidCallback onChanged;

  const PcPicker({
    super.key,
    required this.found,
    required this.searching,
    required this.selected,
    required this.enabled,
    required this.host,
    required this.port,
    required this.onPick,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (found.isEmpty)
          Row(
            children: [
              if (searching)
                const SizedBox(
                  width: 14,
                  height: 14,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              else
                Icon(Icons.search_off, size: 16, color: theme.hintColor),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  searching
                      ? 'Looking for your PC on this network…'
                      : 'Nothing answered. Check Earshot is running on the PC, '
                          'or type its address.',
                  style: theme.textTheme.bodySmall
                      ?.copyWith(color: theme.hintColor),
                ),
              ),
            ],
          )
        else
          ...found.map((f) {
            final isSelected = selected != null &&
                selected!.host == f.destination.host;
            return Padding(
              padding: const EdgeInsets.only(bottom: 6),
              child: InkWell(
                onTap: enabled ? () => onPick(f.destination) : null,
                borderRadius: BorderRadius.circular(8),
                child: Padding(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
                  child: Row(
                    children: [
                      Icon(
                        isSelected
                            ? Icons.radio_button_checked
                            : Icons.radio_button_unchecked,
                        size: 18,
                        color: isSelected
                            ? theme.colorScheme.primary
                            : theme.hintColor,
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: Text(
                          f.destination.host,
                          style: theme.textTheme.bodyLarge?.copyWith(
                            fontWeight: isSelected
                                ? FontWeight.w600
                                : FontWeight.w400,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                      ),
                      // How fast it answered. Not decoration: on a network where two machines
                      // answer, the quick one is the one on this side of the router.
                      Text(
                        '${f.answeredIn.inMilliseconds} ms',
                        style: theme.textTheme.bodySmall
                            ?.copyWith(color: theme.hintColor),
                      ),
                    ],
                  ),
                ),
              ),
            );
          }),
        if (searching && found.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text('Still looking…',
                style:
                    theme.textTheme.bodySmall?.copyWith(color: theme.hintColor)),
          ),
        const SizedBox(height: 6),
        _AddressFields(
          host: host,
          port: port,
          enabled: enabled,
          onChanged: onChanged,
        ),
      ],
    );
  }
}

/// Where to type an address the sweep could not find — another subnet, a VPN, an unusual port.
class _AddressFields extends StatelessWidget {
  final TextEditingController host;
  final TextEditingController port;
  final bool enabled;
  final VoidCallback onChanged;
  const _AddressFields({
    required this.host,
    required this.port,
    required this.enabled,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          flex: 3,
          child: TextField(
            controller: host,
            enabled: enabled,
            keyboardType: TextInputType.url,
            autocorrect: false,
            onChanged: (_) => onChanged(),
            decoration: const InputDecoration(
              labelText: 'Address',
              hintText: '192.168.1.20',
            ),
          ),
        ),
        const SizedBox(width: 10),
        Expanded(
          flex: 2,
          child: TextField(
            controller: port,
            enabled: enabled,
            keyboardType: TextInputType.number,
            onChanged: (_) => onChanged(),
            decoration: const InputDecoration(labelText: 'Port'),
          ),
        ),
      ],
    );
  }
}

