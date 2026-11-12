function [kinematic_flag, drive_flag, force_moment_flag, preload_flag, activeBushNames] = fun_initializeFlagParameters(excitations, bush_prim, bush_sec, bush_add)
    % Initialisierung der kinematic_flag
    kinematic_flag = initializeKinematicFlag(bush_prim, bush_sec);
    
    % Initialisierung der drive_flag
    drive_flag = initializeDriveFlag(excitations.relonomic_drives);
    
    % Initialisierung der force_moment_flag
    force_moment_flag = initializeForceMomentFlag(excitations.forces_and_moments);

    % Initialisierung der activeBushNames
    activeBushNames = collectActiveBushNames(bush_prim, bush_sec);

    % Initialization of preload_flag
    preload_flag = initializePreloadFlag(bush_add);

end

function flag = initializeKinematicFlag(bush_prim, bush_sec)
    flag = true;
    bushes = {bush_prim, bush_sec};
    for k = 1:length(bushes)
        bush_current = bushes{k};
        for i = 1:length(bush_current)
            if bush_current(i).active_flag == 1
                flag = false;
                return;
            end
        end
    end
end

function flag = initializeDriveFlag(relonomic_drives)
    flag = false;
    fields = fieldnames(relonomic_drives);
    for k = 1:numel(fields)
        field_name = fields{k};
        array_data = relonomic_drives.(field_name);
        if any(any(array_data ~= 0))
            flag = true;
            return;
        end
    end
end

function flag = initializeForceMomentFlag(forces_and_moments)
    flag = false;
    fields = fieldnames(forces_and_moments);
    for k = 1:numel(fields)
        field_name = fields{k};
        array_data = forces_and_moments.(field_name);
        if any(any(array_data ~= 0))
            flag = true;
            return;
        end
    end
end

function activeBushNames = collectActiveBushNames(bush_prim, bush_sec)
    activeBushNames = {};  
    bushes = {bush_prim, bush_sec};  
    for k = 1:length(bushes)
        bush_current = bushes{k};
        for i = 1:length(bush_current)
            if bush_current(i).active_flag == 1
                activeBushNames{end+1} = bush_current(i).name; 
            end
        end
    end
    activeBushNames = unique(activeBushNames, 'stable');
end

function flag = initializePreloadFlag(bush_add)
    flag = false;
    for i = 1:length(bush_add)
        if any(bush_add(i).preload ~= 0)
            flag = true;
            return;
        end
    end
end

