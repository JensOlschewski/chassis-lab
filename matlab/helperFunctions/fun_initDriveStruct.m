%FUN_INITDRIVESTRUCT Initializes structures for drive systems.
%
% Initializes structures for relonomic and force-torque drives using information
% from a JSON structure and solver parameters, including signal structures and
% drive vectors, and checks for preloads.
%
% Inputs:
%   jsonStruct   - Structure from JSON with drive details.
%   solverStruct - Contains time array and body segment count.
%
% Output:
%   driveStruct  - Initialized drive and signal structures.

function driveStruct = fun_initDriveStruct(jsonStruct, solverStruct)
    % Initialize signal and drive structures for relonomic and force-torque drives
    signalReloStruct = createSignalStruct(jsonStruct.drives.relonomic_drives, solverStruct.time_array, 'SplineDiff');
    driveReloStruct = createDriveVec(jsonStruct.drives.relonomic_drives, signalReloStruct, solverStruct.numB);
    signalForceStruct = createSignalStruct(jsonStruct.drives.force_torque_drives, solverStruct.time_array, 'SplineDiff');
    driveForceStruct = createDriveVec(jsonStruct.drives.force_torque_drives, signalForceStruct, solverStruct.numB);

    % Setup driveStruct with potential preload information
    driveStruct = struct('ispreload', any(driveForceStruct.signalVec(:,1)), ...
                         'preloadVec', [], ...
                         'signalReloStruct', signalReloStruct, ...
                         'signalForceStruct', signalForceStruct, ...
                         'driveReloStruct', driveReloStruct, ...
                         'driveForceStruct', driveForceStruct);
                     
    % If there's a preload, store the preload vector
    if driveStruct.ispreload
        driveStruct.preloadVec = driveForceStruct.signalVec(:,1);
    end
end
